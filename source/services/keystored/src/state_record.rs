// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: keystored statefs record codecs — Integrity envelopes for the
//!   device-key record and the scoped key-value shim (TASK-0025 step 4)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal; value format SSOT is `statefs::envelope`)
//! TEST_COVERAGE: tests/state_record_contract.rs (roundtrip, legacy, stale-seq,
//!   malformed negatives)
//!
//! PUBLIC API:
//!   - DEVICE_KEY_PATH / SUBJECT / PURPOSE_DEVICE_KEY / PURPOSE_SCOPED
//!   - seal_device_key / open_device_key: 32-byte seed <-> Integrity envelope
//!   - seal_scoped / open_scoped: scoped KV values <-> Integrity envelope
//!
//! `/state/keystore/*` sits on the Integrity floor (alg = none, monotonic
//! seq — the chicken-egg rule: the device-key bootstrap read must never
//! depend on a MAC key derived from that very record). Reads accept BOTH
//! envelope v1 and pre-migration legacy raw bytes; writes are always
//! enveloped with seq = last_seen + 1 (first write: seq = 1).
//!
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use statefs::writer::{self, StoredIntegrity};
use statefs::StatefsError;

/// Storage path of the device signing-key record.
pub const DEVICE_KEY_PATH: &str = "/state/keystore/device.signing";
/// Envelope meta subject for every keystored-authored record.
pub const SUBJECT: &str = "keystored";
/// Envelope meta purpose for the device-key record.
pub const PURPOSE_DEVICE_KEY: &str = "device-key";
/// Envelope meta purpose for the sender-scoped key-value shim records.
pub const PURPOSE_SCOPED: &str = "scoped-kv";

/// Seal the 32-byte device-key seed as an Integrity envelope.
pub fn seal_device_key(seq: u64, ts: u64, seed: &[u8; 32]) -> Result<Vec<u8>, StatefsError> {
    writer::seal_integrity(DEVICE_KEY_PATH, seq, SUBJECT, PURPOSE_DEVICE_KEY, ts, seed)
}

/// Open a stored device-key record: envelope v1 (payload must be exactly
/// 32 bytes) or legacy raw 32 bytes. Any other shape is `Corrupted` —
/// deterministic, never a panic (journal bytes are untrusted input).
pub fn open_device_key(bytes: &[u8]) -> Result<([u8; 32], Option<u64>), StatefsError> {
    let stored = writer::open_stored(bytes)?;
    let payload = stored.payload();
    if payload.len() != 32 {
        return Err(StatefsError::Corrupted);
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(payload);
    Ok((seed, stored.seq()))
}

/// Seal a scoped key-value shim record as an Integrity envelope.
pub fn seal_scoped(path: &str, seq: u64, ts: u64, value: &[u8]) -> Result<Vec<u8>, StatefsError> {
    writer::seal_integrity(path, seq, SUBJECT, PURPOSE_SCOPED, ts, value)
}

/// Open a stored scoped record: envelope payload, or the legacy raw bytes.
pub fn open_scoped(bytes: &[u8]) -> Result<(&[u8], Option<u64>), StatefsError> {
    let stored: StoredIntegrity<'_> = writer::open_stored(bytes)?;
    Ok((stored.payload(), stored.seq()))
}
