// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::{ffi::OsStr, fs, path::PathBuf};

/// Build script:
/// - Scans `tools/nexus-idl/schemas` for all `.capnp` files
/// - Invokes the Cap'n Proto compiler to generate Rust sources into OUT_DIR
/// - Emits proper `rerun-if-changed` hints so Cargo rebuilds when schemas change
fn main() {
    // Resolve the schema directory relative to this crate (robust in workspaces/CI).
    let schemas: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/nexus-idl/schemas");

    // If no schema directory exists, there's nothing to generate. Keep the build green.
    if !schemas.exists() {
        println!(
            "cargo:warning=nexus-idl-runtime: no schemas at {}",
            schemas.display()
        );
        return;
    }

    // Re-run the build script if the directory or any schema file changes.
    println!("cargo:rerun-if-changed={}", schemas.display());
    for entry in fs::read_dir(&schemas).expect("read schemas dir") {
        let path = entry.expect("dirent").path();
        if path.extension() == Some(OsStr::new("capnp")) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    // Generate Rust code for all `.capnp` files in the schema directory.
    // `capnpc` writes to Cargo's OUT_DIR by default; `src_prefix` preserves include paths.
    let mut cmd = capnpc::CompilerCommand::new();
    cmd.src_prefix(&schemas);

    for entry in fs::read_dir(&schemas).expect("read schemas dir") {
        let path = entry.expect("dirent").path();
        if path.extension() == Some(OsStr::new("capnp")) {
            cmd.file(&path);
        }
    }

    if let Err(e) = cmd.run() {
        panic!(
            "capnp compile failed: {e}\n\
             Hint: install `capnp` (>= 0.5.2). On Debian/Ubuntu: `apt-get install capnproto`."
        );
    }
}
