// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Build script for config effective Cap'n Proto bindings.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `cargo test -p nexus-config -- --nocapture`.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

#![allow(clippy::expect_used)]

use std::{env, path::PathBuf};

fn main() {
    let schema = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/nexus-idl/schemas/config_effective.capnp");
    let schema_dir =
        schema.parent().map(PathBuf::from).expect("config_effective schema parent missing");

    println!("cargo:rerun-if-changed={}", schema.display());

    capnpc::CompilerCommand::new()
        .src_prefix(&schema_dir)
        .file(&schema)
        .run()
        .expect("capnp compile failed for config_effective schema");
}
