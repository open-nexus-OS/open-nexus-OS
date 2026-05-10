// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Build script for `neuron-boot` linker arguments and boot-map emission.
//! OWNERS: @kernel-team
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered indirectly by `cargo build -p neuron-boot` and QEMU boot proofs.
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let linker_script = manifest_dir.join("kernel.ld");
    println!("cargo:rerun-if-changed={}", linker_script.display());
    // Use canonicalize to ensure only a single absolute path reaches the linker
    let abs_script = linker_script.canonicalize().expect("kernel.ld must exist");
    println!("cargo:rustc-link-arg=-T{}", abs_script.display());

    let repo_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .expect("neuron-boot must live under source/kernel");
    let map_path = repo_root.join("neuron-boot.map");
    println!("cargo:rustc-link-arg=-Map={}", map_path.display());
}
