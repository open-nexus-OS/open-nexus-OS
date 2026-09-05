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

// Note: deny(unsafe_code) instead of forbid to allow generated capnp code
// which uses unsafe for RawStructSchema initialization (capnp >= 0.24).
#![deny(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(feature = "os-lite", not(feature = "std")))]
extern crate alloc;

#[cfg(all(not(feature = "std"), not(feature = "os-lite")))]
compile_error!("Either 'std' or 'os-lite' feature must be enabled");

// Generated Cap'n Proto bindings - allow clippy lints we don't control.
#[allow(unsafe_code, clippy::unwrap_used, clippy::needless_lifetimes)]
pub mod system_set_capnp {
    include!(concat!(env!("OUT_DIR"), "/system_set_capnp.rs"));
}

/// RFC-0089 §12.4 system-volume assembler (TASK-0321 P3).
pub mod bundle_delta;
pub mod component_set;
pub mod delta_apply;
pub mod stage_journal;
pub mod system_set;
pub mod volume_apply;

/// Device publisher trust anchor, baked at build time from
/// `policies/update-trust.toml` (RFC-0089 §4, TASK-0198 Phase 1 — the nxra
/// `BAKED_TRUST` pattern). Host tests inject their own allowlists; the OS
/// verification path passes `trust::BAKED_PUBLISHERS`.
pub mod trust {
    include!(concat!(env!("OUT_DIR"), "/publishers_baked.rs"));
}

/// A/B slot id — kept here for the SystemSet surface; the boot-control
/// MACHINE relocated to `bootctld` (TASK-0050, ADR-0055).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    pub fn other(self) -> Self {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }
}
#[cfg(feature = "std")]
pub use system_set::Ed25519Verifier;
pub use system_set::{
    BundleRecord, SignatureVerifier, SystemSet, SystemSetError, SystemSetIndex, VerifyError,
};
