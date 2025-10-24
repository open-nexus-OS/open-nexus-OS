//! CONTEXT: Build script for Cap'n Proto IDL code generation
//!
//! OWNERS: @runtime
//!
//! BUILD PROCESS:
//!   - Scans `tools/nexus-idl/schemas` for all `.capnp` files
//!   - Invokes Cap'n Proto compiler to generate Rust sources into OUT_DIR
//!   - Emits proper `rerun-if-changed` hints for Cargo rebuilds
//!   - Falls back to manual bindings if capnp compiler unavailable
//!
//! DEPENDENCIES:
//!   - capnpc: Cap'n Proto compiler crate
//!   - std::fs: File system operations
//!   - std::env: Environment variable access
//!   - std::path: Path handling
//!
//! FEATURES:
//!   - Automatic schema discovery and compilation
//!   - Manual fallback for CI environments
//!   - Proper Cargo integration with rerun hints
//!   - Error handling with helpful messages
//!
//! ERROR CONDITIONS:
//!   - Schema directory not found: Warning only, build continues
//!   - Capnp compiler unavailable: Falls back to manual bindings
//!   - Manual fallback fails: Panic with installation instructions
//!
//! ADR: docs/adr/0004-idl-runtime-architecture.md

use std::{env, ffi::OsStr, fs, io, path::Path, path::PathBuf};

fn main() {
    // Resolve the schema directory relative to this crate (robust in workspaces/CI).
    let schemas: PathBuf =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/nexus-idl/schemas");

    // If no schema directory exists, there's nothing to generate. Keep the build green.
    if !schemas.exists() {
        println!("cargo:warning=nexus-idl-runtime: no schemas at {}", schemas.display());
        return;
    }

    // Re-run the build script if the directory or any schema file changes.
    println!("cargo:rerun-if-changed={}", schemas.display());
    if let Ok(entries) = fs::read_dir(&schemas) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension() == Some(OsStr::new("capnp")) {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    if let Err(err) = generate_with_capnpc(&schemas) {
        if err.contains("Failed to execute `capnp --version`") {
            if let Err(copy_err) = fallback_to_manual() {
                panic!(
                    "capnp compile failed: {err}; manual fallback failed: {copy_err}\n\
                     Hint: install `capnp` (>= 0.5.2). On Debian/Ubuntu: `apt-get install capnproto`."
                );
            }
        } else {
            panic!(
                "capnp compile failed: {err}\n\
                 Hint: install `capnp` (>= 0.5.2). On Debian/Ubuntu: `apt-get install capnproto`."
            );
        }
    }
}

fn generate_with_capnpc(schemas: &Path) -> Result<(), String> {
    let mut cmd = capnpc::CompilerCommand::new();
    cmd.src_prefix(schemas);

    let entries = fs::read_dir(schemas).map_err(|err| err.to_string())?;
    for entry in entries {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension() == Some(OsStr::new("capnp")) {
            cmd.file(&path);
        }
    }

    cmd.run().map_err(|err| err.to_string())
}

fn fallback_to_manual() -> Result<(), io::Error> {
    let manual_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/manual");
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);

    if !manual_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("manual schemas missing at {}", manual_dir.display()),
        ));
    }

    println!(
        "cargo:warning=nexus-idl-runtime: capnp compiler unavailable, using bundled manual bindings"
    );
    println!("cargo:rerun-if-changed={}", manual_dir.display());

    for entry in fs::read_dir(&manual_dir)? {
        let path = entry?.path();
        if path.extension() == Some(OsStr::new("rs")) {
            println!("cargo:rerun-if-changed={}", path.display());
            let Some(file_name) = path.file_name() else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "manual file without name",
                ));
            };
            let target = out_dir.join(file_name);
            fs::copy(&path, &target)?;
        }
    }

    Ok(())
}
