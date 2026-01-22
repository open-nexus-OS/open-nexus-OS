// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Update domain library (system-set parsing + RAM-based boot control)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 11 integration tests (via tests/updates_host)
//!   - system-set parsing and signature verification
//!   - BootCtrl state machine and error states
//!   - security: path-traversal rejection
//!
//! PUBLIC API:
//!   - BootCtrl: in-memory A/B slot state machine
//!   - SystemSet: verified `.nxs` archive model
//!
//! DEPENDENCIES:
//!   - capnp: system-set index decoding
//!   - sha2: bundle digest verification
//!   - ed25519-dalek (std): host signature verification
//!
//! ADR: docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(feature = "os-lite", not(feature = "std")))]
extern crate alloc;

#[cfg(all(not(feature = "std"), not(feature = "os-lite")))]
compile_error!("Either 'std' or 'os-lite' feature must be enabled");

// Generated Cap'n Proto bindings - allow clippy lints we don't control.
#[allow(clippy::unwrap_used, clippy::needless_lifetimes)]
pub mod system_set_capnp {
    include!(concat!(env!("OUT_DIR"), "/system_set_capnp.rs"));
}

pub mod bootctrl;
pub mod system_set;

pub use bootctrl::{BootCtrl, BootCtrlError, Slot};
#[cfg(feature = "std")]
pub use system_set::Ed25519Verifier;
pub use system_set::{
    BundleRecord, SignatureVerifier, SystemSet, SystemSetError, SystemSetIndex, VerifyError,
};
