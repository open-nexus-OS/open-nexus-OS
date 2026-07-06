// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Compiles the canonical Scene-IR schema (ui_ir.capnp) for the typed
//! zero-copy wrappers in this crate. The schema SSOT lives with all other IDL
//! schemas under tools/nexus-idl/schemas (ADR-0021).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (build-time only)
//! TEST_COVERAGE: No tests (build script)

use std::path::PathBuf;

fn main() {
    let schema = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tools/nexus-idl/schemas/ui_ir.capnp");
    println!("cargo:rerun-if-changed={}", schema.display());

    let prefix = schema.parent().map(PathBuf::from).unwrap_or_default();
    if let Err(err) = capnpc::CompilerCommand::new().src_prefix(&prefix).file(&schema).run() {
        panic!(
            "ui_ir.capnp compile failed: {err}\n\
             Hint: install `capnp` (>= 0.5.2). On many distros the package is `capnproto`."
        );
    }
}
