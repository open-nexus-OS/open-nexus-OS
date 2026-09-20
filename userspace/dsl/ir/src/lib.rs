// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(feature = "std"), no_std)]
// Generated capnp code needs unsafe for its schema tables; everything else stays safe.
#![deny(unsafe_code)]

//! CONTEXT: `nexus-dsl-ir` — typed zero-copy access to the canonical Scene IR
//! (`.nxir`, `tools/nexus-idl/schemas/ui_ir.capnp`) plus the determinism kernel:
//! bounded reading, schema version gating, canonical `programHash`, and stable
//! view-node identity. `no_std`+alloc so the same reader runs on the host, in the
//! compositor mount, and in the app-host process.
//! OWNERS: @ui @runtime
//! STATUS: In progress (TASK-0075)
//! API_STABILITY: Unstable (schema v1.0)
//! TEST_COVERAGE: unit tests + host suite `tests/dsl_v0_1a_host`
//! DOCS: docs/dev/dsl/ir.md (schema changelog)

extern crate alloc;

// capnp-generated code; restriction lints (unwrap/expect) are not ours to fix.
#[allow(
    unsafe_code,
    clippy::all,
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    missing_docs
)]
pub mod ui_ir_capnp {
    include!(concat!(env!("OUT_DIR"), "/ui_ir_capnp.rs"));
}

#[cfg(feature = "hash-verify")]
pub mod hashing;
pub mod node_id;
pub mod read;
pub mod validate;

/// Schema major version this crate reads/writes. Readers reject other majors.
pub const SCHEMA_MAJOR: u16 = 1;
/// Schema minor version this crate writes. Readers accept any minor of
/// [`SCHEMA_MAJOR`], because `SCHEMA_MAJOR` gates only the major — so a minor
/// is a documented contract rather than an enforced gate, and everything
/// in-tree builds together.
///
/// * Minor 4 — `UiProgram.window` (WindowIntent). Additive.
/// * Minor 5 — slots (RFC-0084): `Component.slots`, `ViewNode.slot`,
///   `ComponentRef.slots`. Additive, but honestly one-way: an old reader
///   meeting a program that USES `slot` gets `NotInSchema` →
///   [`IrError::Malformed`], so such a program requires a runtime at minor ≥ 5.
/// * Minor 6 — retires `CallStep.timeoutMs` (TASK-0077B): a service call has no
///   client clock. Subtractive, and safe only because everything in-tree builds
///   together — the ordinal @3 is burned rather than reused.
/// * Minor 7 — `Store.keyed` (TASK-0077B): per-instance local state, keyed by
///   the node identity the IR already persists. Additive.
/// * Minor 8 — `Handler.bind` carries `BindWrite { target, value }`
///   (TASK-0077B): a two-way bind states HOW the interaction produces the
///   value, so the runtime executes a rule instead of guessing from the widget
///   kind. `Handler.bind @4` is retired as `retiredBind`, ordinal burned.
pub const SCHEMA_MINOR: u16 = 8;

/// Byte length of `programHash` / `sourceDigest` (SHA-256).
pub const DIGEST_LEN: usize = 32;

/// Errors surfaced by bounded reading + structural validation.
///
/// Stable, matchable codes — the app-host maps these to deterministic launch
/// errors; nothing here formats into user-facing strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrError {
    /// Message decoding failed (truncated/corrupt/over the traversal budget).
    Malformed,
    /// `schemaVersionMajor` differs from [`SCHEMA_MAJOR`].
    UnsupportedMajor,
    /// A digest field has the wrong length.
    BadDigest,
    /// `programHash` does not match the canonical bytes.
    HashMismatch,
    /// A cross-reference index points outside its table.
    DanglingRef,
    /// A declared budget is exceeded by the program's own contents.
    BudgetExceeded,
    /// Symbol table is not sorted/unique.
    SymbolsNotCanonical,
    /// A typed expression fails re-typechecking on load.
    TypeMismatch,
}
