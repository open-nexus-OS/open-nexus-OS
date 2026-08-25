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

// Build scripts may use expect/unwrap since failures are hard errors.
#![allow(clippy::expect_used)]

use std::fmt::Write as _;
use std::{env, path::PathBuf};

// Shared narrow parser (also exercised by tests/updates_host/tests/trust_parse.rs).
include!("build_trust.rs");

fn main() {
    let schema = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/nexus-idl/schemas/system-set.capnp");
    let schema_dir = schema.parent().map(PathBuf::from).expect("system-set schema parent missing");

    println!("cargo:rerun-if-changed={}", schema.display());

    capnpc::CompilerCommand::new()
        .src_prefix(&schema_dir)
        .file(&schema)
        .run()
        .expect("capnp compile failed for system-set schema");

    bake_publisher_trust();
}

// RFC-0089 §4 / TASK-0198 Phase 1: bake `policies/update-trust.toml` into
// `OUT_DIR/publishers_baked.rs` (nxra BAKED_TRUST pattern). Every violation
// FAILS THE BUILD — a half-parsed trust list must never produce a permissive
// anchor.
fn bake_publisher_trust() {
    let trust_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../policies/update-trust.toml");
    println!("cargo:rerun-if-changed={}", trust_path.display());
    let text = std::fs::read_to_string(&trust_path)
        .unwrap_or_else(|err| panic!("update-trust.toml unreadable: {err}"));
    let keys = parse_update_trust(&text)
        .unwrap_or_else(|err| panic!("update-trust.toml: {err} (build fails closed)"));

    let mut out = String::new();
    out.push_str("/// Build-time publisher trust anchor from policies/update-trust.toml\n");
    out.push_str("/// (RFC-0089 §4). Membership is checked BEFORE any signature use.\n");
    out.push_str("pub static BAKED_PUBLISHERS: &[[u8; 32]] = &[\n");
    for key in &keys {
        let mut bytes = String::new();
        for byte in key {
            let _ = write!(bytes, "{byte:#04x}, ");
        }
        let _ = writeln!(out, "    [{bytes}],");
    }
    out.push_str("];\n");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR unset");
    std::fs::write(PathBuf::from(&out_dir).join("publishers_baked.rs"), out)
        .expect("write publishers_baked.rs");
}
