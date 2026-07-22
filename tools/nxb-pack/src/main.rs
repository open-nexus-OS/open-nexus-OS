//! CONTEXT: Nexus bundle packer tool
//! INTENT: Package ELF binaries into deployable bundles with manifest
//! IDL (target): pack(input_elf, output_dir), packHello(output_dir)
//! DEPS: exec-payloads (hello ELF), std::fs (file operations)
//! READINESS: Command-line tool; no service dependencies
//! TESTS: Pack hello ELF; pack custom ELF; validate deterministic SBOM embedding
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use exec_payloads::{HELLO_ELF, HELLO_MANIFEST_NXB};
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let first = args.next().ok_or_else(|| usage("missing input ELF path"))?;
    let mut toml_path: Option<String> = None;
    let mut hello = false;
    let (input, output) = match first.as_str() {
        "--hello" => {
            hello = true;
            let output = args.next().ok_or_else(|| usage("missing output directory"))?;
            (None, output)
        }
        "--toml" => {
            toml_path = Some(args.next().ok_or_else(|| usage("missing manifest.toml path"))?);
            let input = args.next().ok_or_else(|| usage("missing input ELF path"))?;
            let output = args.next().ok_or_else(|| usage("missing output directory"))?;
            (Some(input), output)
        }
        _ => {
            let output = args.next().ok_or_else(|| usage("missing output directory"))?;
            (Some(first), output)
        }
    };
    if args.next().is_some() {
        return Err(usage("too many arguments"));
    }

    let input_path = input.as_ref().map(Path::new);
    if let Some(path) = input_path {
        if !path.is_file() {
            return Err(format!("input ELF not found: {}", path.display()).into());
        }
    }

    let output_dir = PathBuf::from(output);
    let manifest_bytes = if hello {
        HELLO_MANIFEST_NXB.to_vec()
    } else if let Some(toml_path) = toml_path.as_deref() {
        let toml_str = fs::read_to_string(toml_path)?;
        compile_toml_to_manifest_nxb(&toml_str)?
    } else {
        // Default deterministic manifest for bring-up (unsigned placeholder).
        default_manifest_nxb()
    };
    let payload_bytes =
        if let Some(path) = input_path { fs::read(path)? } else { HELLO_ELF.to_vec() };

    pack_bundle(&output_dir, &manifest_bytes, &payload_bytes)?;

    Ok(())
}

fn pack_bundle(
    output_dir: &Path,
    manifest_bytes: &[u8],
    payload_bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(output_dir)?;
    let manifest_with_payload =
        rewrite_manifest_with_digests(manifest_bytes, payload_bytes, None, None)?;
    let manifest_binding_sha256 = sha256_hex(&manifest_with_payload);
    // v2.1: DSL bundles ship `payload.nxir`; native bundles keep `payload.elf`
    // (digest/size fields are payload-agnostic — bundlemgrd stays unchanged).
    let payload_name = if manifest_payload_is_ui_program(&manifest_with_payload)? {
        "payload.nxir"
    } else {
        "payload.elf"
    };
    fs::write(output_dir.join(payload_name), payload_bytes)?;
    let sbom_bytes =
        write_sbom(output_dir, &manifest_with_payload, payload_bytes, &manifest_binding_sha256)?;
    let repro_bytes =
        write_repro(output_dir, payload_bytes, &sbom_bytes, &manifest_binding_sha256)?;
    let final_manifest = rewrite_manifest_with_digests(
        &manifest_with_payload,
        payload_bytes,
        Some(&sbom_bytes),
        Some(&repro_bytes),
    )?;
    fs::write(output_dir.join("manifest.nxb"), final_manifest)?;
    Ok(())
}

fn write_sbom(
    output_dir: &Path,
    manifest_bytes: &[u8],
    payload_bytes: &[u8],
    manifest_binding_sha256: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let manifest = parse_manifest_info(manifest_bytes)?;
    let input = sbom::BundleSbomInput {
        bundle_name: manifest.name,
        bundle_version: manifest.semver,
        publisher_hex: manifest.publisher_hex,
        payload_sha256: sha256_hex(payload_bytes),
        payload_size: payload_bytes.len() as u64,
        manifest_sha256: manifest_binding_sha256.to_string(),
        source_date_epoch: sbom::source_date_epoch_from_env()?,
        components: Vec::new(),
    };
    let sbom_json = sbom::generate_bundle_sbom_json(&input)?;
    let meta_dir = output_dir.join("meta");
    fs::create_dir_all(&meta_dir)?;
    fs::write(meta_dir.join("sbom.json"), &sbom_json)?;
    Ok(sbom_json)
}

fn write_repro(
    output_dir: &Path,
    payload_bytes: &[u8],
    sbom_bytes: &[u8],
    manifest_binding_sha256: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let repro_json = repro::capture_bundle_repro_json_with_manifest_digest(
        manifest_binding_sha256,
        payload_bytes,
        sbom_bytes,
    )?;
    let meta_dir = output_dir.join("meta");
    fs::create_dir_all(&meta_dir)?;
    fs::write(meta_dir.join("repro.env.json"), &repro_json)?;
    Ok(repro_json)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

struct ManifestInfo {
    name: String,
    semver: String,
    publisher_hex: String,
}

fn parse_manifest_info(bytes: &[u8]) -> Result<ManifestInfo, Box<dyn std::error::Error>> {
    use capnp::message::ReaderOptions;
    use nexus_idl_runtime::manifest_capnp::bundle_manifest;
    let mut cursor = std::io::Cursor::new(bytes);
    let message = capnp::serialize::read_message(&mut cursor, ReaderOptions::new())?;
    let reader = message.get_root::<bundle_manifest::Reader<'_>>()?;

    let name = reader.get_name()?.to_str()?.trim().to_string();
    let semver = reader.get_semver()?.to_str()?.trim().to_string();
    let publisher = reader.get_publisher()?;
    if publisher.len() != 16 {
        return Err(format!("manifest publisher must be 16 bytes, got {}", publisher.len()).into());
    }
    Ok(ManifestInfo { name, semver, publisher_hex: hex::encode(publisher) })
}

fn rewrite_manifest_with_digests(
    manifest_bytes: &[u8],
    payload_bytes: &[u8],
    sbom_bytes: Option<&[u8]>,
    repro_bytes: Option<&[u8]>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use capnp::message::{Builder, ReaderOptions};
    use nexus_idl_runtime::manifest_capnp::bundle_manifest;

    let mut cursor = std::io::Cursor::new(manifest_bytes);
    let message = capnp::serialize::read_message(&mut cursor, ReaderOptions::new())?;
    let src = message.get_root::<bundle_manifest::Reader<'_>>()?;

    let mut builder = Builder::new_default();
    {
        let mut dst = builder.init_root::<bundle_manifest::Builder<'_>>();
        dst.set_schema_version(src.get_schema_version());
        dst.set_name(src.get_name()?.to_str()?);
        dst.set_semver(src.get_semver()?.to_str()?);
        dst.set_min_sdk(src.get_min_sdk()?.to_str()?);
        dst.set_publisher(src.get_publisher()?);
        dst.set_signature(src.get_signature()?);
        dst.set_payload_digest(&hex::decode(sha256_hex(payload_bytes))?);
        dst.set_payload_size(payload_bytes.len() as u64);
        // v2.0/v2.1 scalars must survive the digest rewrite (payloadKind
        // decides the packed payload filename; bundleType feeds the registry).
        // NOTE (pre-existing gap): the v2.0 LIST fields (dependencies/
        // providedServices/resources) are still dropped here — none of the
        // shipped bundles use them yet; copy them when the first one does.
        if let Ok(kind) = src.get_payload_kind() {
            dst.set_payload_kind(kind);
        }
        if let Ok(bundle_type) = src.get_bundle_type() {
            dst.set_bundle_type(bundle_type);
        }

        if let Some(sbom) = sbom_bytes {
            dst.set_sbom_digest(&hex::decode(sha256_hex(sbom))?);
        } else {
            dst.set_sbom_digest(&[]);
        }
        if let Some(repro) = repro_bytes {
            dst.set_repro_digest(&hex::decode(sha256_hex(repro))?);
        } else {
            dst.set_repro_digest(&[]);
        }

        let src_abilities = src.get_abilities()?;
        let mut dst_abilities = dst.reborrow().init_abilities(src_abilities.len());
        for idx in 0..src_abilities.len() {
            dst_abilities.set(idx, src_abilities.get(idx)?.to_str()?);
        }

        let src_caps = src.get_capabilities()?;
        let mut dst_caps = dst.reborrow().init_capabilities(src_caps.len());
        for idx in 0..src_caps.len() {
            dst_caps.set(idx, src_caps.get(idx)?.to_str()?);
        }
        // v2.2: exports survive the digest rewrite (the v2.0 LIST fields
        // below still drop until first use — recorded gap).
        let src_exports = src.get_exports()?;
        let mut dst_exports = dst.reborrow().init_exports(src_exports.len());
        for idx in 0..src_exports.len() {
            let se = src_exports.get(idx);
            let mut de = dst_exports.reborrow().get(idx);
            de.set_ability(se.get_ability()?.to_str()?);
            de.set_permission(se.get_permission()?.to_str()?);
        }
    }

    let mut out: Vec<u8> = Vec::new();
    capnp::serialize::write_message(&mut out, &builder)?;
    Ok(out)
}

/// Reads `payloadKind` back from encoded manifest bytes (v2.1; older
/// manifests decode to the `elf` default).
fn manifest_payload_is_ui_program(
    manifest_bytes: &[u8],
) -> Result<bool, Box<dyn std::error::Error>> {
    use nexus_idl_runtime::manifest_capnp::{self as mf, bundle_manifest};
    let reader = capnp::serialize::read_message(manifest_bytes, Default::default())?;
    let manifest = reader.get_root::<bundle_manifest::Reader<'_>>()?;
    Ok(matches!(manifest.get_payload_kind(), Ok(mf::PayloadKind::UiProgram)))
}

fn compile_toml_to_manifest_nxb(input: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use capnp::message::Builder;
    use nexus_idl_runtime::manifest_capnp::{self as mf, bundle_manifest};
    use toml::Value;

    fn req_str<'a>(
        table: &'a toml::Table,
        key: &'static str,
    ) -> Result<&'a str, Box<dyn std::error::Error>> {
        table
            .get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("manifest.toml missing/invalid `{key}`").into())
    }

    fn opt_str<'a>(table: &'a toml::Table, key: &'static str) -> Option<&'a str> {
        table.get(key).and_then(|v| v.as_str())
    }

    fn opt_str_array(
        table: &toml::Table,
        key: &'static str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let Some(v) = table.get(key) else {
            return Ok(Vec::new());
        };
        let arr = v
            .as_array()
            .ok_or_else(|| format!("manifest.toml `{key}` must be an array").to_string())?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let s = item.as_str().ok_or_else(|| {
                format!("manifest.toml `{key}` entries must be strings").to_string()
            })?;
            out.push(s.to_string());
        }
        Ok(out)
    }

    let root: Value = toml::from_str(input)?;
    let table = root.as_table().ok_or_else(|| "manifest.toml root must be a table".to_string())?;

    // Accept existing key names used throughout the repo:
    // - version -> semver
    // - caps -> capabilities
    // - min_sdk -> minSdk
    let name = req_str(table, "name")?.trim();
    let semver = req_str(table, "version")?.trim();
    let min_sdk = req_str(table, "min_sdk")?.trim();
    let abilities = opt_str_array(table, "abilities")?;
    let capabilities = opt_str_array(table, "caps")?;

    // Publisher/signature are hex strings in TOML input; tool allows zero placeholders.
    let publisher_hex = opt_str(table, "publisher").unwrap_or("00000000000000000000000000000000");
    let sig_hex = opt_str(table, "sig").unwrap_or("00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
    let publisher = hex::decode(publisher_hex.trim())?;
    let signature = hex::decode(sig_hex.trim())?;

    if publisher.len() != 16 {
        return Err(format!(
            "manifest.toml `publisher` must decode to 16 bytes, got {}",
            publisher.len()
        )
        .into());
    }
    if signature.len() != 64 {
        return Err(format!(
            "manifest.toml `sig` must decode to 64 bytes, got {}",
            signature.len()
        )
        .into());
    }

    // v2.1: payload kind (TASK-0080D). Default `elf`; DSL apps declare
    // `payload_kind = "ui-program"` and ship `payload.nxir`.
    let payload_kind = match opt_str(table, "payload_kind").unwrap_or("elf") {
        "elf" => mf::PayloadKind::Elf,
        "ui-program" => mf::PayloadKind::UiProgram,
        other => return Err(format!("manifest.toml unknown payload_kind: {other}").into()),
    };

    // v2.0: bundle type (default app for backward compat). `shell`/`greeter`
    // are system-role UI bundles (TASK-0080C) — the type is a PRIVILEGE CEILING
    // enforced below.
    let bundle_type = match opt_str(table, "bundle_type").unwrap_or("app") {
        "app" => mf::BundleType::App,
        "service" => mf::BundleType::Service,
        "library" => mf::BundleType::Library,
        "driver" => mf::BundleType::Driver,
        "framework" => mf::BundleType::Framework,
        "shell" => mf::BundleType::Shell,
        "greeter" => mf::BundleType::Greeter,
        "settings" => mf::BundleType::Settings,
        "filemanager" => mf::BundleType::Filemanager,
        "ime" => mf::BundleType::Ime,
        other => return Err(format!("manifest.toml unknown bundle_type: {other}").into()),
    };

    // Privilege ceiling (TASK-0080C): a permission gated on a system role is
    // only grantable to a bundle of that role's type. This is the security
    // boundary — a plain `app` cannot ship `SESSION`/`LAUNCH`/`ENUMERATE`, so a
    // rogue app can never drive login or launch/enumerate apps. Fail-closed at
    // PACK time (the earliest point), before a bundle can ever ship. The
    // product manifest still ASSIGNS which bundle plays the role.
    for cap in &capabilities {
        let required = match cap.as_str() {
            "nexus.permission.SESSION" => Some(("greeter", mf::BundleType::Greeter)),
            "nexus.permission.LAUNCH" | "nexus.permission.ENUMERATE" => {
                Some(("shell", mf::BundleType::Shell))
            }
            "nexus.permission.SETTINGS" => Some(("settings", mf::BundleType::Settings)),
            "nexus.permission.FILES" => Some(("filemanager", mf::BundleType::Filemanager)),
            "nexus.permission.IME" => Some(("ime", mf::BundleType::Ime)),
            _ => None,
        };
        if let Some((role, needed)) = required {
            if bundle_type != needed {
                return Err(format!(
                    "manifest.toml capability `{cap}` requires bundle_type = \"{role}\" \
                     (a normal app may not hold a system-role permission)"
                )
                .into());
            }
        }
    }

    // v2.0: dependencies
    let deps_toml = opt_str_array(table, "dependencies")?;
    // v2.0: provided services
    let provides = opt_str_array(table, "provides")?;
    // v2.0: resources
    let res_toml = opt_str_array(table, "resources")?;
    // v2.2: app-to-app exports (TASK-0081): array of inline tables
    // `{ ability = "chat.Send", permission = "app.chat.SEND" }`. The
    // permission MUST live in the app's OWN namespace `app.<name>.<CAP>` —
    // validated HERE so a foreign-namespace export never ships (fail-closed
    // at pack time, not at launch time).
    let mut exports_toml: Vec<(String, String)> = Vec::new();
    if let Some(value) = table.get("exports") {
        let items = value
            .as_array()
            .ok_or_else(|| "manifest.toml `exports` must be an array".to_string())?;
        for item in items {
            let entry = item
                .as_table()
                .ok_or_else(|| "manifest.toml `exports` entries must be tables".to_string())?;
            let ability = entry
                .get("ability")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "export missing `ability`".to_string())?;
            let permission = entry
                .get("permission")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "export missing `permission`".to_string())?;
            let own_prefix = format!("app.{name}.");
            if !permission.starts_with(&own_prefix) || permission.len() == own_prefix.len() {
                return Err(format!(
                    "export permission `{permission}` must be `{own_prefix}<CAP>` (app-owned namespace)"
                )
                .into());
            }
            exports_toml.push((ability.to_string(), permission.to_string()));
        }
    }

    let mut builder = Builder::new_default();
    let mut msg = builder.init_root::<bundle_manifest::Builder>();
    msg.set_schema_version(2);
    msg.set_name(name);
    msg.set_semver(semver);
    msg.set_min_sdk(min_sdk);
    msg.set_publisher(&publisher);
    msg.set_signature(&signature);
    msg.set_bundle_type(bundle_type);
    msg.set_payload_kind(payload_kind);

    {
        let mut a = msg.reborrow().init_abilities(abilities.len() as u32);
        for (i, s) in abilities.iter().enumerate() {
            a.set(i as u32, s);
        }
    }
    {
        let mut c = msg.reborrow().init_capabilities(capabilities.len() as u32);
        for (i, s) in capabilities.iter().enumerate() {
            c.set(i as u32, s);
        }
    }
    // v2.0: dependencies (simple name-only format: "name" or "name@^1.0")
    {
        let mut deps = msg.reborrow().init_dependencies(deps_toml.len() as u32);
        for (i, dep_str) in deps_toml.iter().enumerate() {
            let (dep_name, dep_ver) = if let Some(at) = dep_str.find('@') {
                (&dep_str[..at], &dep_str[at + 1..])
            } else {
                (dep_str.as_str(), "*")
            };
            let mut dep = deps.reborrow().get(i as u32);
            dep.set_name(dep_name);
            dep.set_version_constraint(dep_ver);
        }
    }
    // v2.0: provided services
    {
        let mut p = msg.reborrow().init_provided_services(provides.len() as u32);
        for (i, s) in provides.iter().enumerate() {
            p.set(i as u32, s);
        }
    }
    // v2.2: exports
    {
        let mut ex = msg.reborrow().init_exports(exports_toml.len() as u32);
        for (i, (ability, permission)) in exports_toml.iter().enumerate() {
            let mut e = ex.reborrow().get(i as u32);
            e.set_ability(ability);
            e.set_permission(permission);
        }
    }
    // v2.0: resources (format: "kind:path", e.g. "icon:icons/app.svg")
    {
        let mut r = msg.reborrow().init_resources(res_toml.len() as u32);
        for (i, res_str) in res_toml.iter().enumerate() {
            let (kind_str, res_path) = if let Some(colon) = res_str.find(':') {
                (&res_str[..colon], &res_str[colon + 1..])
            } else {
                ("data", res_str.as_str())
            };
            let kind = match kind_str {
                "icon" => mf::ResourceKind::Icon,
                "cursor" => mf::ResourceKind::Cursor,
                "font" => mf::ResourceKind::Font,
                "wallpaper" => mf::ResourceKind::Wallpaper,
                "sound" => mf::ResourceKind::Sound,
                _ => mf::ResourceKind::Data,
            };
            let mut res = r.reborrow().get(i as u32);
            res.set_path(res_path);
            res.set_kind(kind);
        }
    }

    let mut out: Vec<u8> = Vec::new();
    capnp::serialize::write_message(&mut out, &builder)?;
    Ok(out)
}

fn default_manifest_nxb() -> Vec<u8> {
    // Keep bring-up deterministic: a fixed manifest for non-hello bundles when no TOML is provided.
    // This is intentionally unsigned placeholder data (all-zero publisher/signature).
    let toml = r#"
name = "demo.unnamed"
version = "0.0.0"
abilities = ["demo"]
caps = []
min_sdk = "0.1.0"
bundle_type = "app"
dependencies = []
provides = []
resources = []
publisher = "00000000000000000000000000000000"
sig = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
"#;
    compile_toml_to_manifest_nxb(toml).unwrap_or_else(|_| Vec::new())
}

fn usage(message: &str) -> Box<dyn std::error::Error> {
    let mut stderr = io::stderr();
    let _ = writeln!(stderr, "nxb-pack: {message}");
    let _ = writeln!(stderr, "usage: nxb-pack <input.elf> <output.nxb>");
    let _ = writeln!(stderr, "   or: nxb-pack --hello <output.nxb>");
    let _ = writeln!(stderr, "   or: nxb-pack --toml <manifest.toml> <input.elf> <output.nxb>");
    Box::<dyn std::error::Error>::from(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supply_chain_sbom_is_deterministic() {
        let first = tempfile::tempdir().expect("first tempdir");
        let second = tempfile::tempdir().expect("second tempdir");
        let manifest = default_manifest_nxb();
        let payload = vec![0xde, 0xad, 0xbe, 0xef];

        pack_bundle(first.path(), &manifest, &payload).expect("first bundle pack");
        pack_bundle(second.path(), &manifest, &payload).expect("second bundle pack");

        let first_sbom = fs::read(first.path().join("meta").join("sbom.json")).expect("first sbom");
        let second_sbom =
            fs::read(second.path().join("meta").join("sbom.json")).expect("second sbom");
        let first_repro =
            fs::read(first.path().join("meta").join("repro.env.json")).expect("first repro");
        let second_repro =
            fs::read(second.path().join("meta").join("repro.env.json")).expect("second repro");

        assert_eq!(first_sbom, second_sbom);
        assert_eq!(first_repro, second_repro);
        let sbom_text = String::from_utf8(first_sbom).expect("sbom utf8");
        assert!(sbom_text.contains("\"specVersion\":\"1.5\""));
        assert!(sbom_text.contains("\"nexus.payload.sha256\""));
        let manifest_for_binding = rewrite_manifest_with_digests(&manifest, &payload, None, None)
            .expect("binding manifest");
        let expected = repro::ReproVerifyInput {
            payload_sha256: sha256_hex(&payload),
            manifest_sha256: sha256_hex(&manifest_for_binding),
            sbom_sha256: sha256_hex(sbom_text.as_bytes()),
        };
        repro::verify_repro_json(&first_repro, &expected).expect("repro verification must pass");
    }
}
