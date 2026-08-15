// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS client-side Integrity-envelope helpers (TASK-0025 step 4)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (client convenience; the value format SSOT is `envelope`)
//! TEST_COVERAGE: Host unit tests (envelope/legacy classification, seq discipline)
//!
//! PUBLIC API:
//!   - StoredIntegrity: classification of bytes read back under an Integrity key
//!   - open_stored: envelope-or-legacy read helper (deterministic, no panic)
//!   - next_seq: read-modify-write seq discipline (first write = seq 1)
//!   - seal_integrity: Integrity-class (alg = none) seal shorthand
//!
//! Writers under the boot-critical Integrity floor (`/state/keystore/*`,
//! `/state/boot/*`) share one read-modify-write contract with statefsd's
//! verify-on-put: read the stored value, learn its seq, write seq =
//! last_seen + 1. Legacy raw bytes (pre-migration journals) carry no seq —
//! they classify as `Legacy` and the next write starts at seq = 1, which
//! statefsd accepts because raw values never feed its replay tracker.
//! CHICKEN-EGG RULE: Integrity class needs no MAC key (alg = none), so the
//! keystored device-key bootstrap never depends on derived key material.
//!
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::envelope::{self, EnvelopeMeta, PolicyClass, ENVELOPE_MAGIC};
use super::StatefsError;

/// A stored value under an Integrity-class key, as read back from statefsd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredIntegrity<'a> {
    /// Pre-migration raw bytes (no envelope magic): served unchanged.
    Legacy(&'a [u8]),
    /// Envelope v1: unwrapped payload plus its monotonic seq.
    Enveloped {
        /// The wrapped value payload.
        payload: &'a [u8],
        /// The envelope's monotonic per-key sequence number.
        seq: u64,
    },
}

impl<'a> StoredIntegrity<'a> {
    /// The effective value bytes (envelope payload, or the raw legacy bytes).
    pub fn payload(&self) -> &'a [u8] {
        match self {
            Self::Legacy(bytes) => bytes,
            Self::Enveloped { payload, .. } => payload,
        }
    }

    /// The stored seq, if the value was enveloped (legacy carries none).
    pub fn seq(&self) -> Option<u64> {
        match self {
            Self::Legacy(_) => None,
            Self::Enveloped { seq, .. } => Some(*seq),
        }
    }
}

/// Classify bytes read back under an Integrity-class key. Bytes without the
/// envelope magic are legacy raw values (pre-migration journals); magic-bearing
/// bytes must decode as a structurally valid envelope (`Corrupted` otherwise —
/// deterministic, never a panic; bounded parsing lives in `envelope::decode`).
pub fn open_stored(bytes: &[u8]) -> Result<StoredIntegrity<'_>, StatefsError> {
    if bytes.len() < ENVELOPE_MAGIC.len() || bytes[..ENVELOPE_MAGIC.len()] != ENVELOPE_MAGIC {
        return Ok(StoredIntegrity::Legacy(bytes));
    }
    let env = envelope::decode(bytes)?;
    Ok(StoredIntegrity::Enveloped { payload: env.payload, seq: env.seq })
}

/// Next seq for a write: strictly above the last seen one. A first write
/// (no envelope stored yet, or a legacy raw value) starts at seq = 1.
pub fn next_seq(last_seen: Option<u64>) -> u64 {
    last_seen.unwrap_or(0).saturating_add(1)
}

/// Bounded capacity of a `SeqCache` (distinct keys).
pub const MAX_CACHED_KEYS: usize = 256;

/// Writer-side memory of the last seq written per key. Needed because the
/// stored value alone cannot carry the high-water mark across a DELETE:
/// statefsd's replay-fed tracker (correctly) keeps the max-seen seq for a
/// deleted key, so a re-put that learned "no stored value -> seq 1" would be
/// rejected as a rollback forever. `next_for` therefore takes the max of the
/// stored seq and the cache; `note_written` records every accepted write.
/// Delete must NOT evict an entry. Bounded: at capacity, unknown keys are
/// simply not cached (their writers fall back to stored-seq learning).
#[derive(Debug, Default)]
pub struct SeqCache {
    last_written: BTreeMap<String, u64>,
}

impl SeqCache {
    /// Create an empty cache.
    pub fn new() -> Self {
        Self { last_written: BTreeMap::new() }
    }

    /// Seq to use for the next write of `key`: strictly above both the
    /// stored envelope's seq (if any) and the last seq this writer wrote.
    pub fn next_for(&self, key: &str, stored: Option<u64>) -> u64 {
        let cached = self.last_written.get(key).copied();
        let last_seen = match (stored, cached) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        next_seq(last_seen)
    }

    /// Record an accepted write of `key` at `seq` (monotonic; bounded).
    pub fn note_written(&mut self, key: &str, seq: u64) {
        match self.last_written.get_mut(key) {
            Some(entry) => {
                if seq > *entry {
                    *entry = seq;
                }
            }
            None => {
                if self.last_written.len() < MAX_CACHED_KEYS {
                    self.last_written.insert(String::from(key), seq);
                }
            }
        }
    }
}

/// Seal `payload` as an Integrity-class envelope (alg = none, no MAC key —
/// the chicken-egg rule for boot-critical prefixes).
pub fn seal_integrity(
    key_path: &str,
    seq: u64,
    subject: &str,
    purpose: &str,
    ts: u64,
    payload: &[u8],
) -> Result<Vec<u8>, StatefsError> {
    let meta = EnvelopeMeta { subject, purpose, ts };
    envelope::seal(PolicyClass::Integrity, key_path, seq, meta, payload, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "/state/boot/bootctl.v1";

    #[test]
    fn test_open_stored_roundtrip_enveloped() {
        let sealed = seal_integrity(KEY, 3, "updated", "bootctl", 42, b"abc").unwrap();
        match open_stored(&sealed).unwrap() {
            StoredIntegrity::Enveloped { payload, seq } => {
                assert_eq!(payload, b"abc");
                assert_eq!(seq, 3);
            }
            StoredIntegrity::Legacy(_) => panic!("expected envelope"),
        }
    }

    #[test]
    fn test_open_stored_legacy_raw_passthrough() {
        let stored = open_stored(b"raw-bytes").unwrap();
        assert_eq!(stored.payload(), b"raw-bytes");
        assert_eq!(stored.seq(), None);
    }

    #[test]
    fn test_reject_malformed_magic_bearing_bytes() {
        // Envelope magic followed by garbage must be a deterministic error,
        // never a legacy fallback (that would mask tampering).
        let mut bytes = Vec::from(&ENVELOPE_MAGIC[..]);
        bytes.extend_from_slice(&[0xff; 7]);
        assert_eq!(open_stored(&bytes).unwrap_err(), StatefsError::Corrupted);
    }

    #[test]
    fn test_next_seq_discipline() {
        assert_eq!(next_seq(None), 1); // first write / legacy upgrade
        assert_eq!(next_seq(Some(7)), 8); // read-modify-write
        assert_eq!(next_seq(Some(u64::MAX)), u64::MAX); // saturating
    }

    #[test]
    fn test_seq_cache_survives_delete_then_reput() {
        // The regression shape: put (seq 1) -> delete -> re-put. The stored
        // value is gone, but the server tracker still holds max = 1; only
        // the cache can produce the required seq 2.
        let mut cache = SeqCache::new();
        assert_eq!(cache.next_for(KEY, None), 1);
        cache.note_written(KEY, 1);
        // Delete: no cache eviction. Re-put sees stored = None.
        assert_eq!(cache.next_for(KEY, None), 2);
    }

    #[test]
    fn test_seq_cache_takes_max_of_stored_and_cached() {
        let mut cache = SeqCache::new();
        cache.note_written(KEY, 2);
        // Another writer advanced the stored record past our cache.
        assert_eq!(cache.next_for(KEY, Some(5)), 6);
        // Cache ahead of a stale stored read.
        cache.note_written(KEY, 9);
        assert_eq!(cache.next_for(KEY, Some(5)), 10);
    }
}
