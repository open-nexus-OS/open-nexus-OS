// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Build-time Cap'n Proto schema generation for TASK-0056 v2a host proofs.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Stable for TASK-0056 proof floor
//! TEST_COVERAGE: Covered by `cargo test -p ui_v2a_host -- --nocapture`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

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
