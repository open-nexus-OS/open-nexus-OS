//! CONTEXT: Nexus bundle packer tool
//! INTENT: Package ELF binaries into deployable bundles with manifest
//! IDL (target): pack(input_elf, output_dir), packHello(output_dir)
//! DEPS: exec-payloads (hello ELF), std::fs (file operations)
//! READINESS: Command-line tool; no service dependencies
//! TESTS: Pack hello ELF; pack custom ELF; validate output structure
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use exec_payloads::{HELLO_ELF, HELLO_MANIFEST_NXB};

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
    fs::create_dir_all(&output_dir)?;

    let manifest_path = output_dir.join("manifest.nxb");
    if hello {
        fs::write(&manifest_path, HELLO_MANIFEST_NXB)?;
    } else if let Some(toml_path) = toml_path.as_deref() {
        let toml_str = fs::read_to_string(toml_path)?;
        let bytes = compile_toml_to_manifest_nxb(&toml_str)?;
        fs::write(&manifest_path, bytes)?;
    } else {
        // Default deterministic manifest for bring-up (unsigned placeholder).
        fs::write(&manifest_path, default_manifest_nxb())?;
    }

    let payload_path = output_dir.join("payload.elf");
    if let Some(path) = input_path {
        fs::copy(path, &payload_path)?;
    } else {
        fs::write(&payload_path, HELLO_ELF)?;
    }

    Ok(())
}

fn compile_toml_to_manifest_nxb(input: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use capnp::message::Builder;
    use nexus_idl_runtime::manifest_capnp::bundle_manifest;
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

    let mut builder = Builder::new_default();
    let mut msg = builder.init_root::<bundle_manifest::Builder>();
    msg.set_schema_version(1);
    msg.set_name(name);
    msg.set_semver(semver);
    msg.set_min_sdk(min_sdk);
    msg.set_publisher(&publisher);
    msg.set_signature(&signature);

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
