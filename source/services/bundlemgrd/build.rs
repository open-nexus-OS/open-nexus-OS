// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// Build scripts fail by panicking (unwrap/expect) — the correct failure mode
// for build-time codegen; the restriction lints target runtime code only.
#![allow(clippy::expect_used, clippy::unwrap_used)]

//! CONTEXT: Build script — generates `APP_REGISTRY` from the real bundle
//! manifests so the Apps menu reflects installed bundles, not a const (RFC-0065).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (build script; the registry is exercised by bundlemgrd `os_lite` tests)
//!
//! Build script: generates the installed-app registry (`APP_REGISTRY`) from the
//! REAL app bundle manifests (`userspace/apps/<app>/manifest.toml`) instead of a
//! hand-maintained const in source (RFC-0065). The manifests are the single
//! source of truth: an app appears in the Apps menu iff it has a bundle manifest,
//! so a phantom entry (e.g. an app that does not exist) cannot drift in. The
//! generated table is `include!`d by `os_lite.rs`.
//!
//! This is build-time enumeration of the shipped bundle set (appropriate for the
//! no_std service, which cannot read a filesystem at runtime). The future install
//! pipeline feeds the embedded `.nxb` set through this same table.

use std::path::PathBuf;

// TASK-0321 (RFC-0089 §12.3, ADR-0060): bundlemgrd verifies the system
// volume's NXSV against the SAME OS-image anchor the loader holds
// (policies/os-trust.toml) — baked at build time through the shared narrow
// parser; a malformed file fails the build (no permissive anchor, ever).
include!("../../../userspace/updates/build_trust.rs");

fn bake_os_trust() {
    use std::fmt::Write as _;
    let trust_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../policies/os-trust.toml");
    println!("cargo:rerun-if-changed={}", trust_path.display());
    let text = std::fs::read_to_string(&trust_path)
        .unwrap_or_else(|err| panic!("os-trust.toml unreadable: {err}"));
    let keys = parse_update_trust(&text)
        .unwrap_or_else(|err| panic!("os-trust.toml: {err} (build fails closed)"));
    let mut out = String::new();
    out.push_str("/// Build-time OS-image trust anchor from policies/os-trust.toml\n");
    out.push_str("/// (RFC-0089 §12.3). NXSV signatures verify against these keys ONLY.\n");
    out.push_str("pub(crate) static BAKED_OS_KEYS: &[[u8; 32]] = &[\n");
    for key in &keys {
        let mut bytes = String::new();
        for byte in key {
            let _ = write!(bytes, "{byte:#04x}, ");
        }
        let _ = writeln!(out, "    [{bytes}],");
    }
    out.push_str("];\n");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out_dir.join("os_trust_baked.rs"), out).expect("write os_trust_baked.rs");
}

fn main() {
    bake_os_trust();
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
    // TASK-0321 P5: the app registry + ui-program payloads are no longer
    // baked into this binary — they live on the verified system volume
    // (`meta/app.properties` + `payload.elf` per app bundle, packed by
    // `nx app compile` + `nxb-pack` in scripts/build.sh).
}
