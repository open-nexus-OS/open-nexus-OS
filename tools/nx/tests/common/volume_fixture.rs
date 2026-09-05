// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared fixture for the `nx image` system-volume integration
//! tests (TASK-0321 / TASK-0035): deterministic seeds, a fake kernel, tiny
//! RISC-V ELFs with a real `.sdata`, bundle directories (ADR-0020 layout)
//! and the `nx image build [--system-bundles]` invocations. Used by
//! image_volume_cli.rs and image_reuse_cli.rs.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: helper module (no tests of its own)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

#![allow(dead_code)]

use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

pub const OS_SEED_HEX: &str = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b";
pub const PUB_SEED_HEX: &str = "0707070707070707070707070707070707070707070707070707070707070707";

pub fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("nx process must run")
}

pub fn setup(dir: &Path) {
    std::fs::write(dir.join("os.seed"), OS_SEED_HEX).expect("os seed");
    std::fs::write(dir.join("pub.seed"), PUB_SEED_HEX).expect("pub seed");
    let kernel: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(dir.join("kernel.bin"), kernel).expect("kernel");
}

/// Streams the file (384 MiB disk images — never slurped in a test).
pub fn file_sha(path: &Path) -> [u8; 32] {
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

pub fn build(dir: &Path, out: &str, build_id: &str) -> Output {
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
pub fn manifest_nxb(name: &str, version: &str) -> Vec<u8> {
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
pub fn tiny_elf(sdata_addr: u64) -> Vec<u8> {
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

pub fn patch_sdata_addr(elf: &mut [u8], addr: u64) {
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

pub fn make_bundle(root: &Path, name: &str, version: &str, spawnable: bool, payload: &[u8]) {
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

pub fn setup_bundles(dir: &Path) {
    let root = dir.join("bundles");
    make_bundle(&root, "metricsd", "1.0.0", true, &tiny_elf(0x8004_0000));
    make_bundle(&root, "timed", "1.0.0", true, &tiny_elf(0x8008_0000));
    make_bundle(&root, "apps", "1.0.0", false, b"ui-program-bytes");
}

pub fn build_with_volume(dir: &Path, out: &str, build_id: &str) -> Output {
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
