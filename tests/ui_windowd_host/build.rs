// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let idl_dir = manifest_dir.join("../../source/services/windowd/idl");

    println!("cargo:rerun-if-changed={}", idl_dir.display());

    capnpc::CompilerCommand::new()
        .src_prefix(&idl_dir)
        .file(idl_dir.join("surface.capnp"))
        .file(idl_dir.join("layer.capnp"))
        .file(idl_dir.join("vsync.capnp"))
        .file(idl_dir.join("input.capnp"))
        .run()?;
    Ok(())
}
