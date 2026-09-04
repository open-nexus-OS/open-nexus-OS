// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary tests for the `nx image` system-volume half
//! (TASK-0321 P1, RFC-0089 §12): bundle directories → pkgimg v3 volume +
//! signed NXSV on system-a paired with boot-a (determinism, launch params
//! from `meta/launch.json` + the ELF's `.sdata`), `verify` walking every
//! digest and rejecting tampered / unpaired volumes, and `ota --bundle-set`
//! emitting `[boot-image, system-volume, bundle…]` whose kind-2 payloads
//! ARE the index bundle windows.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 integration tests (incl. patch re-pairing)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

const OS_SEED_HEX: &str = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b";
const PUB_SEED_HEX: &str = "0707070707070707070707070707070707070707070707070707070707070707";

fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("nx process must run")
}

fn setup(dir: &Path) {
    std::fs::write(dir.join("os.seed"), OS_SEED_HEX).expect("os seed");
    std::fs::write(dir.join("pub.seed"), PUB_SEED_HEX).expect("pub seed");
    let kernel: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(dir.join("kernel.bin"), kernel).expect("kernel");
}

/// Streams the file (384 MiB disk images — never slurped in a test).
fn file_sha(path: &Path) -> [u8; 32] {
    use std::io::Read;
    let mut f = std::fs::File::open(path).expect("open");
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).expect("read");
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    hasher.finalize().into()
}

fn build(dir: &Path, out: &str, build_id: &str) -> Output {
    run_nx(
        &[
            "image",
            "build",
            "--kernel",
            "kernel.bin",
            "--out",
            out,
            "--sign",
            "os.seed",
            "--build-id",
            build_id,
        ],
        dir,
    )
}
// ---------------------------------------------------------- system volume --
// TASK-0321 P1 (RFC-0089 §12): bundle directories → pkgimg v3 volume +
// signed NXSV on system-a, paired with boot-a; verify walks every digest;
// `ota --bundle-set` emits `[boot-image, system-volume, bundle…]`.

/// A canonical `manifest.nxb` (ADR-0020) for `name@version`.
fn manifest_nxb(name: &str, version: &str) -> Vec<u8> {
    use nexus_idl_runtime::manifest_capnp::bundle_manifest;
    let mut message = capnp::message::Builder::new_default();
    let mut m = message.init_root::<bundle_manifest::Builder>();
    m.set_schema_version(2);
    m.set_name(name);
    m.set_semver(version);
    m.set_min_sdk("0.1.0");
    m.set_publisher(&[0u8; 16]);
    m.set_signature(&[0u8; 64]);
    let mut abilities = m.reborrow().init_abilities(1);
    abilities.set(0, "service");
    let mut out = Vec::new();
    capnp::serialize::write_message(&mut out, &message).expect("manifest capnp");
    out
}

/// A minimal RISC-V ELF carrying a `.sdata` section at a known address, so
/// the volume builder derives `global_pointer = .sdata + 0x800` exactly the
/// way init-lite's build script does.
fn tiny_elf(sdata_addr: u64) -> Vec<u8> {
    use object::write::{Object, SectionKind, StandardSection};
    let mut obj = Object::new(
        object::BinaryFormat::Elf,
        object::Architecture::Riscv64,
        object::Endianness::Little,
    );
    let text = obj.section_id(StandardSection::Text);
    obj.append_section_data(text, &[0x13, 0x00, 0x00, 0x00], 4); // nop
    let sdata = obj.add_section(Vec::new(), b".sdata".to_vec(), SectionKind::Data);
    obj.append_section_data(sdata, &[1, 2, 3, 4, 5, 6, 7, 8], 8);
    obj.section_mut(sdata).flags = object::SectionFlags::Elf { sh_flags: 0x3 };
    let mut bytes = obj.write().expect("elf");
    // object::write emits a relocatable object (section addresses 0); patch
    // the .sdata sh_addr so the psABI fallback sees a real address.
    patch_sdata_addr(&mut bytes, sdata_addr);
    bytes
}

fn patch_sdata_addr(elf: &mut [u8], addr: u64) {
    // ELF64 LE: e_shoff@0x28, e_shentsize@0x3a, e_shnum@0x3c, e_shstrndx@0x3e.
    let shoff = u64::from_le_bytes(elf[0x28..0x30].try_into().unwrap()) as usize;
    let shentsize = u16::from_le_bytes(elf[0x3a..0x3c].try_into().unwrap()) as usize;
    let shnum = u16::from_le_bytes(elf[0x3c..0x3e].try_into().unwrap()) as usize;
    let shstrndx = u16::from_le_bytes(elf[0x3e..0x40].try_into().unwrap()) as usize;
    let str_off = {
        let sh = shoff + shstrndx * shentsize;
        u64::from_le_bytes(elf[sh + 0x18..sh + 0x20].try_into().unwrap()) as usize
    };
    for i in 0..shnum {
        let sh = shoff + i * shentsize;
        let name_off = u32::from_le_bytes(elf[sh..sh + 4].try_into().unwrap()) as usize;
        let name_end = elf[str_off + name_off..].iter().position(|&b| b == 0).unwrap();
        if &elf[str_off + name_off..str_off + name_off + name_end] == b".sdata" {
            elf[sh + 0x10..sh + 0x18].copy_from_slice(&addr.to_le_bytes());
        }
    }
}

fn make_bundle(root: &Path, name: &str, version: &str, spawnable: bool, payload: &[u8]) {
    let dir = root.join(name);
    std::fs::create_dir_all(dir.join("meta")).expect("bundle dir");
    std::fs::write(dir.join("manifest.nxb"), manifest_nxb(name, version)).expect("manifest");
    std::fs::write(dir.join("payload.elf"), payload).expect("payload");
    std::fs::write(dir.join("meta").join("sbom.json"), b"{}").expect("sbom");
    if spawnable {
        std::fs::write(dir.join("meta").join("launch.json"), br#"{ "stack_pages": 8 }"#)
            .expect("launch");
    }
}

fn setup_bundles(dir: &Path) {
    let root = dir.join("bundles");
    make_bundle(&root, "metricsd", "1.0.0", true, &tiny_elf(0x8004_0000));
    make_bundle(&root, "timed", "1.0.0", true, &tiny_elf(0x8008_0000));
    make_bundle(&root, "apps", "1.0.0", false, b"ui-program-bytes");
}

fn build_with_volume(dir: &Path, out: &str, build_id: &str) -> Output {
    run_nx(
        &[
            "image",
            "build",
            "--kernel",
            "kernel.bin",
            "--out",
            out,
            "--sign",
            "os.seed",
            "--build-id",
            build_id,
            "--system-bundles",
            "bundles",
        ],
        dir,
    )
}

#[test]
fn system_volume_build_is_deterministic_and_verifies() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    setup_bundles(dir.path());
    let a = build_with_volume(dir.path(), "a.img", "dev-S");
    assert!(a.status.success(), "build a: {}", String::from_utf8_lossy(&a.stdout));
    assert!(build_with_volume(dir.path(), "b.img", "dev-S").status.success(), "build b");
    assert_eq!(file_sha(&dir.path().join("a.img")), file_sha(&dir.path().join("b.img")));
    assert!(dir.path().join("a.system-a.pkgimg").is_file(), "side file for the budget gate");

    let verify =
        run_nx(&["image", "verify", "--image", "a.img", "--key", "os.seed", "--json"], dir.path());
    assert!(verify.status.success(), "verify: {}", String::from_utf8_lossy(&verify.stdout));
    let v: serde_json::Value = serde_json::from_slice(&verify.stdout).expect("json");
    let sys = &v["data"]["system_a"];
    assert_eq!(sys["build_id"], "dev-S");
    assert_eq!(sys["paired"], true);
    let bundles = sys["bundles"].as_array().expect("bundles");
    assert_eq!(bundles.len(), 3);
    let metricsd = bundles.iter().find(|b| b["bundle"] == "metricsd").expect("metricsd row");
    assert_eq!(metricsd["stack_pages"], 8);
    assert_eq!(metricsd["global_pointer"], "0x80040800", "gp = .sdata + 0x800");
    let apps = bundles.iter().find(|b| b["bundle"] == "apps").expect("apps row");
    assert_eq!(apps["stack_pages"], 0, "data-only bundle is not spawnable");

    // A build WITHOUT bundles reports the slot as absent (factory-empty).
    assert!(build(dir.path(), "plain.img", "dev-P").status.success());
    let verify = run_nx(
        &["image", "verify", "--image", "plain.img", "--key", "os.seed", "--json"],
        dir.path(),
    );
    assert!(verify.status.success());
    let v: serde_json::Value = serde_json::from_slice(&verify.stdout).expect("json");
    assert_eq!(v["data"]["system_a"]["absent"], true);
}

#[test]
fn verify_rejects_tampered_and_unpaired_system_volume() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    setup_bundles(dir.path());
    assert!(build_with_volume(dir.path(), "t.img", "dev-T").status.success());
    let img = dir.path().join("t.img");

    // Layout SSOT: bsb@1MiB, boot-a@2MiB(56MiB), boot-b@58MiB(56MiB),
    // system-a@114MiB; the volume body starts at sector 8. Patch ONE byte
    // in place (the image is 384 MiB — never slurp it in a test).
    let sys_a: u64 = 114 * 1024 * 1024;
    let off = sys_a + 8 * 512 + 8192 + 100; // inside a bundle window
    let flip = |xor: u8| {
        use std::io::{Read, Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&img).expect("open");
        let mut b = [0u8; 1];
        f.seek(SeekFrom::Start(off)).unwrap();
        f.read_exact(&mut b).unwrap();
        f.seek(SeekFrom::Start(off)).unwrap();
        f.write_all(&[b[0] ^ xor]).unwrap();
    };
    flip(0xFF);
    let verify = run_nx(&["image", "verify", "--image", "t.img", "--key", "os.seed"], dir.path());
    assert!(!verify.status.success(), "tampered volume must fail verify");
    let msg = String::from_utf8_lossy(&verify.stdout);
    assert!(msg.contains("digest"), "{msg}");
    flip(0xFF); // restore

    // Unpaired: refresh boot-a with a different image → NXSV pairing breaks.
    let other: Vec<u8> = (0..300_000u32).map(|i| (i % 241) as u8).collect();
    std::fs::write(dir.path().join("other.bin"), other).expect("other kernel");
    let patch = run_nx(
        &[
            "image",
            "patch",
            "--image",
            "t.img",
            "--part",
            "boot-a",
            "--kernel",
            "other.bin",
            "--sign",
            "os.seed",
            "--build-id",
            "dev-T2",
        ],
        dir.path(),
    );
    assert!(patch.status.success(), "patch: {patch:?}");
    let verify = run_nx(&["image", "verify", "--image", "t.img", "--key", "os.seed"], dir.path());
    assert!(!verify.status.success(), "unpaired volume must fail verify");
    assert!(String::from_utf8_lossy(&verify.stdout).contains("pair"));

    // The keep-blk / flasher shape: `patch --system-bundles` refreshes the
    // paired system slot together with the boot slot → verify is green again.
    let patch = run_nx(
        &[
            "image",
            "patch",
            "--image",
            "t.img",
            "--part",
            "boot-a",
            "--kernel",
            "other.bin",
            "--sign",
            "os.seed",
            "--build-id",
            "dev-T2",
            "--system-bundles",
            "bundles",
        ],
        dir.path(),
    );
    assert!(patch.status.success(), "patch+volume: {patch:?}");
    let verify =
        run_nx(&["image", "verify", "--image", "t.img", "--key", "os.seed", "--json"], dir.path());
    assert!(verify.status.success(), "re-paired: {}", String::from_utf8_lossy(&verify.stdout));
    let v: serde_json::Value = serde_json::from_slice(&verify.stdout).expect("json");
    assert_eq!(v["data"]["system_a"]["build_id"], "dev-T2");
    assert_eq!(v["data"]["system_a"]["paired"], true);
}

#[test]
fn bundle_set_container_decodes_kinds_2_and_6() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    setup_bundles(dir.path());
    let ota = run_nx(
        &[
            "image",
            "ota",
            "--kernel",
            "kernel.bin",
            "--out",
            "set.nxs",
            "--sign-publisher",
            "pub.seed",
            "--sign-os",
            "os.seed",
            "--build-id",
            "dev-S",
            "--rollback-index",
            "2",
            "--bundle-set",
            "bundles",
        ],
        dir.path(),
    );
    assert!(ota.status.success(), "ota: {}", String::from_utf8_lossy(&ota.stdout));
    let bytes = std::fs::read(dir.path().join("set.nxs")).expect("read nxs");
    let mut archive = tar::Archive::new(&bytes[..]);
    let mut names = Vec::new();
    let mut contents: Vec<Vec<u8>> = Vec::new();
    for entry in archive.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        names.push(entry.path().expect("path").display().to_string());
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut buf).expect("read entry");
        contents.push(buf);
    }
    assert_eq!(
        names,
        vec![
            "manifest.nxo",
            "manifest.sig.ed25519",
            "boot.img",
            "system.idx",
            "bundles/apps@1.0.0.bin",
            "bundles/metricsd@1.0.0.bin",
            "bundles/timed@1.0.0.bin",
        ],
        "§12.4 ordering: boot image, system-volume, bundles (sorted)"
    );
    let reader = capnp::serialize::read_message(
        &mut contents[0].as_slice(),
        capnp::message::ReaderOptions::new(),
    )
    .expect("capnp read");
    let manifest = reader
        .get_root::<updates::system_set_capnp::component_manifest::Reader>()
        .expect("manifest root");
    let components = manifest.get_components().expect("components");
    assert_eq!(components.len(), 5);
    let kinds: Vec<u8> = (0..components.len()).map(|i| components.get(i).get_kind()).collect();
    assert_eq!(kinds, vec![1, 6, 2, 2, 2]);

    // Kind 6: payload = superblock + index; kindData = NXSV verifying under
    // the OS key, index digest == component digest, paired with boot.img.
    let sv = components.get(1);
    let os_key = ed25519_dalek::SigningKey::from_bytes(&[0x0bu8; 32]).verifying_key().to_bytes();
    let nxsv = bootfmt::nxsv::verify(sv.get_kind_data().expect("nxsv"), &os_key)
        .expect("embedded nxsv verifies");
    let mut h = Sha256::new();
    h.update(&contents[3]);
    let idx_digest: [u8; 32] = h.finalize().into();
    assert_eq!(nxsv.index_sha256, idx_digest);
    assert_eq!(nxsv.index_len as usize, contents[3].len());
    assert_eq!(sv.get_sha256().expect("sha"), &idx_digest[..]);
    assert_eq!(nxsv.rollback_index, 2);
    let mut h = Sha256::new();
    h.update(&contents[2]);
    let boot_digest: [u8; 32] = h.finalize().into();
    assert_eq!(nxsv.boot_image_sha256, boot_digest, "system volume pairs with the boot image");

    // Kind 2: each payload is exactly a bundle window — its digest equals
    // the index bundle-table digest.
    let index =
        storage::pkgimg_bundles::parse_index(&contents[3], &storage::pkgimg::PkgImgCaps::default())
            .expect("index parses standalone");
    for (i, name) in ["apps", "metricsd", "timed"].iter().enumerate() {
        let c = components.get(2 + i as u32);
        assert_eq!(c.get_name().expect("name").to_str().unwrap(), format!("{name}@1.0.0"));
        let row = index.bundle(name, "1.0.0").expect("bundle row");
        let mut h = Sha256::new();
        h.update(&contents[4 + i]);
        let d: [u8; 32] = h.finalize().into();
        assert_eq!(d, row.sha256, "component payload == bundle window");
        assert_eq!(c.get_size(), row.data_len);
    }
}

/// TASK-0321 P3: `image fixtures --system-bundles` emits `bundle-set.nxs`
/// (os-B + system-volume + bundles) and SELF-VERIFIES it through the real
/// device engine against the baked publisher anchor — so the pairing of
/// the NXSV with os-B's digest and the §12.4 ordering are proven at
/// factory time, not first in QEMU.
#[test]
fn fixtures_emit_a_self_verified_bundle_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    let root = dir.path().join("bundles");
    make_bundle(&root, "metricsd", "1.0.1", true, &tiny_elf(0x8004_0000));
    // The device anchor is the repo's dev publisher key (policies/update-
    // trust.toml) — fixtures must verify against exactly that.
    let publisher = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../keys/dev-publisher.ed25519.seed")
        .canonicalize()
        .expect("dev publisher seed");
    let out = run_nx(
        &[
            "image",
            "fixtures",
            "--kernel",
            "kernel.bin",
            "--data-out",
            "data-seed.img",
            "--data-mib",
            "32",
            "--sign-os",
            "os.seed",
            "--sign-publisher",
            publisher.to_str().expect("utf8 path"),
            "--build-id",
            "dev-abcdef",
            "--system-bundles",
            "bundles",
            "--json",
        ],
        dir.path(),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "fixtures: {stdout}");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    let names: Vec<&str> = json["data"]["containers"]
        .as_array()
        .expect("containers")
        .iter()
        .map(|c| c["name"].as_str().expect("name"))
        .collect();
    assert!(names.contains(&"os-B.nxs"), "{names:?}");
    assert!(names.contains(&"bundle-set.nxs"), "{names:?}");
    assert_eq!(json["data"]["build_b"].as_str().expect("build_b"), "otaBabcdef");
    // TASK-0321 P4: with the factory set as `--reuse-from`, only the bundle
    // whose window changed ships; the unchanged one is a device reuse.
    let base = dir.path().join("base");
    make_bundle(&base, "metricsd", "1.0.0", true, &tiny_elf(0x8004_0000));
    make_bundle(&base, "timed", "1.0.0", true, &tiny_elf(0x8008_0000));
    make_bundle(&root, "timed", "1.0.0", true, &tiny_elf(0x8008_0000));
    let out = run_nx(
        &[
            "image",
            "fixtures",
            "--kernel",
            "kernel.bin",
            "--data-out",
            "data-seed3.img",
            "--data-mib",
            "32",
            "--sign-os",
            "os.seed",
            "--sign-publisher",
            publisher.to_str().expect("utf8 path"),
            "--build-id",
            "dev-abcdef",
            "--system-bundles",
            "bundles",
            "--reuse-from",
            "base",
            "--json",
        ],
        dir.path(),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "fixtures: {stdout}");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(json["data"]["bundle_set"]["shipped"], serde_json::json!(["metricsd@1.0.1"]));
    assert_eq!(json["data"]["bundle_set"]["reused"], serde_json::json!(["timed@1.0.0"]));
    // os-B carries the FACTORY volume paired with itself: 2 bundles, both reused.
    assert_eq!(json["data"]["os_b"]["volume_bundles"], 2);
    assert_eq!(json["data"]["os_b"]["reused"], 2);
    // Without the bundle directory no bundle set is emitted (the lane
    // that stages it must fail loudly, never silently stage os-B).
    let out = run_nx(
        &[
            "image",
            "fixtures",
            "--kernel",
            "kernel.bin",
            "--data-out",
            "data-seed2.img",
            "--data-mib",
            "32",
            "--sign-os",
            "os.seed",
            "--sign-publisher",
            publisher.to_str().expect("utf8 path"),
            "--build-id",
            "dev-abcdef",
            "--json",
        ],
        dir.path(),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "fixtures: {stdout}");
    assert!(!stdout.contains("bundle-set.nxs"));
}
