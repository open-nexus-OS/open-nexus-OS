// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Build script for system-set Cap'n Proto bindings
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//!
//! ADR: docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md

use std::{env, path::PathBuf};

fn main() {
    let schema = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/nexus-idl/schemas/system-set.capnp");
    let schema_dir = schema
        .parent()
        .map(PathBuf::from)
        .expect("system-set schema parent missing");

    println!("cargo:rerun-if-changed={}", schema.display());

    capnpc::CompilerCommand::new()
        .src_prefix(&schema_dir)
        .file(&schema)
        .run()
        .expect("capnp compile failed for system-set schema");
}
