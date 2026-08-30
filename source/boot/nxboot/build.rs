// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: nxboot build script — bakes `policies/os-trust.toml` into the
//! loader (`BAKED_OS_KEYS`, RFC-0088 BAKED_TRUST pattern; any parse error
//! FAILS THE BUILD so a half-parsed trust list can never yield a permissive
//! anchor) and wires the bare-metal linker script for the riscv/none target
//! (ADR-0059: link home 0x9200_0000, ≤256 KiB budget asserted in linker.ld).
//! OWNERS: @security @runtime
//! STATUS: Functional
//! TEST_COVERAGE: parser rejects covered by tests/trust_bake.rs (shared fn)
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

// Build scripts may use expect/unwrap since failures are hard errors.
#![allow(clippy::expect_used)]

use std::fmt::Write as _;
use std::{env, path::PathBuf};

// Shared narrow parser (single authority, also used by userspace/updates
// for the publisher anchor and exercised by its host reject tests).
include!("../../../userspace/updates/build_trust.rs");

fn main() {
    bake_os_trust();

    // Bare-metal target: link with the loader's own script so a plain
    // `cargo build -p nxboot --target riscv64imac-unknown-none-elf` is
    // self-contained (no RUSTFLAGS choreography in scripts).
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("linker.ld");
        println!("cargo:rustc-link-arg-bins=-T{}", script.display());
        println!("cargo:rerun-if-changed={}", script.display());
    }
}

fn bake_os_trust() {
    let trust_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../policies/os-trust.toml");
    println!("cargo:rerun-if-changed={}", trust_path.display());
    let text = std::fs::read_to_string(&trust_path)
        .unwrap_or_else(|err| panic!("os-trust.toml unreadable: {err}"));
    let keys = parse_update_trust(&text)
        .unwrap_or_else(|err| panic!("os-trust.toml: {err} (build fails closed)"));

    let mut out = String::new();
    out.push_str("/// Build-time loader trust anchor from policies/os-trust.toml\n");
    out.push_str("/// (RFC-0089 §7). NXBD signatures verify against these keys ONLY.\n");
    out.push_str("pub static BAKED_OS_KEYS: &[[u8; 32]] = &[\n");
    for key in &keys {
        let mut bytes = String::new();
        for byte in key {
            let _ = write!(bytes, "{byte:#04x}, ");
        }
        let _ = writeln!(out, "    [{bytes}],");
    }
    out.push_str("];\n");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR unset");
    std::fs::write(PathBuf::from(&out_dir).join("os_trust_baked.rs"), out)
        .expect("write os_trust_baked.rs");
}
