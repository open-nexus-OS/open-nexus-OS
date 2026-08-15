// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS authenticity envelope v1 (TASK-0025, value-internal format)
//! OWNERS: @runtime
//! STATUS: Functional (host-first; statefsd wiring lands in the next step)
//! API_STABILITY: Stable value format (v1) — journal bytes (RFC-0018) untouched
//! TEST_COVERAGE: Host unit tests incl. tamper/rollback/truncation negatives
//!
//! PUBLIC API:
//!   - PolicyClass: Off / Integrity / Authenticated (per-prefix policy)
//!   - EnvelopeKey: injected 32-byte MAC key (derivation happens in statefsd:
//!     keystored material -> HKDF label "statefs.envelope.v1", NOT here)
//!   - seal / decode / open: wrap, parse, and verify envelopes
//!   - SeqTracker: bounded per-key max-seq tracking (anti-rollback)
//!   - WriteBudget: deterministic latency-budget warn accounting
//!
//! ENVELOPE BINARY LAYOUT (v1, little-endian, fixed struct — no serde/CBOR):
//!   offset size field
//!   0      4    magic "NXEV" (0x4E 0x58 0x45 0x56)
//!   4      1    ver = 0x01
//!   5      1    alg: 0x00 = none (Integrity class), 0x01 = HMAC-SHA256
//!   6      8    seq  (u64, monotonic per key)
//!   14     8    meta.ts (u64, caller-defined clock units)
//!   22     1    meta.subject_len (0..=64)
//!   23     1    meta.purpose_len (0..=64)
//!   24     4    payload_len (u32)
//!   28     ..   subject (UTF-8) | purpose (UTF-8) | hmac[32] iff alg=0x01 | payload
//!   Total length must EXACTLY equal 28 + subject_len + purpose_len + mac_len
//!   + payload_len; every field is bounds-checked before use (no panics).
//!
//! MAC INPUT LAYOUT (HMAC-SHA256, injective via length prefixes):
//!   "NXEV" || ver(1) || alg(1)
//!   || key_path_len(u16 LE) || key_path bytes
//!   || seq(u64 LE) || ts(u64 LE)
//!   || subject_len(u8) || subject || purpose_len(u8) || purpose
//!   || payload_len(u32 LE) || payload
//!   The storage key path is MAC'd but not stored (verifier supplies it), so a
//!   valid envelope cannot be spliced onto a different key.
//!
//! CHICKEN-EGG RULE: boot-critical prefixes (`/state/keystore/*`,
//! `/state/boot/*`) are Integrity class (envelope + seq, no HMAC) because the
//! MAC key derives from keystored material that is not yet readable during
//! their own bootstrap; authenticity there comes from the boot chain
//! (documented, not faked).
//!
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::{StatefsError, MAX_KEY_LEN, MAX_VALUE_SIZE};

type HmacSha256 = Hmac<Sha256>;

// ============================================================================
// Constants (envelope v1)
// ============================================================================

/// Envelope magic: "NXEV" (Nexus Envelope). Defense-in-depth only — the
/// per-prefix policy class decides whether bytes are parsed as an envelope.
pub const ENVELOPE_MAGIC: [u8; 4] = *b"NXEV";
/// Envelope format version.
pub const ENVELOPE_VERSION: u8 = 1;
/// Algorithm: no MAC (Integrity class — envelope + seq only).
pub const ALG_NONE: u8 = 0;
/// Algorithm: HMAC-SHA256 over (key, seq, meta, payload).
pub const ALG_HMAC_SHA256: u8 = 1;
/// Cap for each meta string field (subject, purpose).
pub const MAX_META_FIELD_LEN: usize = 64;
/// HMAC-SHA256 tag length.
pub const HMAC_LEN: usize = 32;
/// Fixed header size: magic(4)+ver(1)+alg(1)+seq(8)+ts(8)+slen(1)+plen(1)+payload_len(4).
pub const ENVELOPE_HEADER_LEN: usize = 28;
/// Bounded SeqTracker capacity (distinct keys).
pub const MAX_TRACKED_KEYS: usize = 4096;

// ============================================================================
// Types
// ============================================================================

/// Injected 32-byte MAC key. Derivation (keystored -> HKDF
/// "statefs.envelope.v1") is wired in statefsd, not here.
pub struct EnvelopeKey(pub [u8; 32]);

impl core::fmt::Debug for EnvelopeKey {
    /// Never leak key material through Debug formatting.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("EnvelopeKey(<redacted>)")
    }
}

/// Per-prefix envelope policy class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyClass {
    /// No envelope: value bytes pass through unchanged (magic is ignored).
    Off,
    /// Envelope + monotonic seq, no HMAC (boot-critical prefixes).
    Integrity,
    /// Envelope + seq + HMAC-SHA256.
    Authenticated,
}

impl PolicyClass {
    /// Default class for a key. statefsd owns the real per-prefix policy
    /// table; this encodes the contract's fixed boot-critical rule.
    pub fn default_for_key(key: &str) -> Self {
        if key.starts_with("/state/keystore/") || key.starts_with("/state/boot/") {
            PolicyClass::Integrity
        } else {
            PolicyClass::Off
        }
    }
}

/// Envelope metadata (bounded; caps enforced at seal and decode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeMeta<'a> {
    /// Writing subject (service id string), <= 64 bytes.
    pub subject: &'a str,
    /// Purpose label, <= 64 bytes.
    pub purpose: &'a str,
    /// Timestamp in caller-defined clock units (injected, deterministic in tests).
    pub ts: u64,
}

/// A decoded (structurally valid) envelope borrowing the input buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope<'a> {
    /// Format version (currently always 1).
    pub ver: u8,
    /// Algorithm byte (`ALG_NONE` or `ALG_HMAC_SHA256`).
    pub alg: u8,
    /// Monotonic per-key sequence number.
    pub seq: u64,
    /// HMAC-SHA256 tag, present iff `alg == ALG_HMAC_SHA256`.
    pub hmac: Option<[u8; HMAC_LEN]>,
    /// Metadata: subject string.
    pub subject: &'a str,
    /// Metadata: purpose string.
    pub purpose: &'a str,
    /// Metadata: timestamp.
    pub ts: u64,
    /// Wrapped value payload.
    pub payload: &'a [u8],
}

// ============================================================================
// Seal (wrap) / decode / verify
// ============================================================================

/// Wrap `payload` for `class`. `Off` returns the payload unchanged;
/// `Authenticated` requires `mac_key` (fail-closed otherwise).
pub fn seal(
    class: PolicyClass,
    key_path: &str,
    seq: u64,
    meta: EnvelopeMeta<'_>,
    payload: &[u8],
    mac_key: Option<&EnvelopeKey>,
) -> Result<Vec<u8>, StatefsError> {
    if key_path.len() > MAX_KEY_LEN {
        return Err(StatefsError::KeyTooLong);
    }
    if meta.subject.len() > MAX_META_FIELD_LEN || meta.purpose.len() > MAX_META_FIELD_LEN {
        return Err(StatefsError::Corrupted);
    }
    let alg = match class {
        PolicyClass::Off => return Ok(payload.to_vec()),
        PolicyClass::Integrity => ALG_NONE,
        PolicyClass::Authenticated => ALG_HMAC_SHA256,
    };
    let mac_len = if alg == ALG_HMAC_SHA256 { HMAC_LEN } else { 0 };
    let total = ENVELOPE_HEADER_LEN
        .saturating_add(meta.subject.len())
        .saturating_add(meta.purpose.len())
        .saturating_add(mac_len)
        .saturating_add(payload.len());
    if total > MAX_VALUE_SIZE {
        return Err(StatefsError::ValueTooLarge);
    }

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&ENVELOPE_MAGIC);
    out.push(ENVELOPE_VERSION);
    out.push(alg);
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&meta.ts.to_le_bytes());
    out.push(meta.subject.len() as u8);
    out.push(meta.purpose.len() as u8);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(meta.subject.as_bytes());
    out.extend_from_slice(meta.purpose.as_bytes());
    if alg == ALG_HMAC_SHA256 {
        // Fail closed: Authenticated class without a key is refused.
        let key = mac_key.ok_or(StatefsError::IntegrityViolation)?;
        let tag = compute_mac(key, key_path, seq, meta, payload)?;
        out.extend_from_slice(&tag);
    }
    out.extend_from_slice(payload);
    Ok(out)
}

/// Structurally decode an envelope. Bounded parsing: every length is checked
/// before use and the total length must match exactly. Malformed input maps
/// to `Corrupted` (wire MALFORMED class); this never panics.
pub fn decode(bytes: &[u8]) -> Result<Envelope<'_>, StatefsError> {
    if bytes.len() < ENVELOPE_HEADER_LEN || bytes[0..4] != ENVELOPE_MAGIC {
        return Err(StatefsError::Corrupted);
    }
    let ver = bytes[4];
    let alg = bytes[5];
    if ver != ENVELOPE_VERSION || (alg != ALG_NONE && alg != ALG_HMAC_SHA256) {
        return Err(StatefsError::Corrupted);
    }
    let mut seq_bytes = [0u8; 8];
    seq_bytes.copy_from_slice(&bytes[6..14]);
    let seq = u64::from_le_bytes(seq_bytes);
    let mut ts_bytes = [0u8; 8];
    ts_bytes.copy_from_slice(&bytes[14..22]);
    let ts = u64::from_le_bytes(ts_bytes);
    let subject_len = bytes[22] as usize;
    let purpose_len = bytes[23] as usize;
    let mut pl_bytes = [0u8; 4];
    pl_bytes.copy_from_slice(&bytes[24..28]);
    let payload_len = u32::from_le_bytes(pl_bytes) as usize;
    if subject_len > MAX_META_FIELD_LEN
        || purpose_len > MAX_META_FIELD_LEN
        || payload_len > MAX_VALUE_SIZE
    {
        return Err(StatefsError::Corrupted);
    }
    let mac_len = if alg == ALG_HMAC_SHA256 { HMAC_LEN } else { 0 };
    let expected = ENVELOPE_HEADER_LEN
        .saturating_add(subject_len)
        .saturating_add(purpose_len)
        .saturating_add(mac_len)
        .saturating_add(payload_len);
    if bytes.len() != expected {
        return Err(StatefsError::Corrupted);
    }
    let subject_end = ENVELOPE_HEADER_LEN + subject_len;
    let purpose_end = subject_end + purpose_len;
    let subject = core::str::from_utf8(&bytes[ENVELOPE_HEADER_LEN..subject_end])
        .map_err(|_| StatefsError::Corrupted)?;
    let purpose = core::str::from_utf8(&bytes[subject_end..purpose_end])
        .map_err(|_| StatefsError::Corrupted)?;
    let (hmac, payload_start) = if alg == ALG_HMAC_SHA256 {
        let mut tag = [0u8; HMAC_LEN];
        tag.copy_from_slice(&bytes[purpose_end..purpose_end + HMAC_LEN]);
        (Some(tag), purpose_end + HMAC_LEN)
    } else {
        (None, purpose_end)
    };
    Ok(Envelope { ver, alg, seq, hmac, subject, purpose, ts, payload: &bytes[payload_start..] })
}

impl Envelope<'_> {
    /// Verify the HMAC-SHA256 tag against `key_path` and `mac_key`
    /// (constant-time compare). Fails if the envelope carries no tag.
    pub fn verify_mac(&self, key_path: &str, mac_key: &EnvelopeKey) -> Result<(), StatefsError> {
        let tag = self.hmac.as_ref().ok_or(StatefsError::IntegrityViolation)?;
        let meta = EnvelopeMeta { subject: self.subject, purpose: self.purpose, ts: self.ts };
        let mut mac = mac_init(mac_key, key_path, self.seq, meta, self.payload)?;
        mac.update(self.payload);
        mac.verify_slice(tag).map_err(|_| StatefsError::IntegrityViolation)
    }
}

/// Build the HMAC state over the fixed MAC input layout, minus the trailing
/// payload bytes (the caller streams those in).
fn mac_init(
    mac_key: &EnvelopeKey,
    key_path: &str,
    seq: u64,
    meta: EnvelopeMeta<'_>,
    payload: &[u8],
) -> Result<HmacSha256, StatefsError> {
    let mut mac =
        HmacSha256::new_from_slice(&mac_key.0).map_err(|_| StatefsError::IntegrityViolation)?;
    mac.update(&ENVELOPE_MAGIC);
    mac.update(&[ENVELOPE_VERSION, ALG_HMAC_SHA256]);
    mac.update(&(key_path.len() as u16).to_le_bytes());
    mac.update(key_path.as_bytes());
    mac.update(&seq.to_le_bytes());
    mac.update(&meta.ts.to_le_bytes());
    mac.update(&[meta.subject.len() as u8]);
    mac.update(meta.subject.as_bytes());
    mac.update(&[meta.purpose.len() as u8]);
    mac.update(meta.purpose.as_bytes());
    mac.update(&(payload.len() as u32).to_le_bytes());
    Ok(mac)
}

/// Compute the full HMAC-SHA256 tag.
fn compute_mac(
    mac_key: &EnvelopeKey,
    key_path: &str,
    seq: u64,
    meta: EnvelopeMeta<'_>,
    payload: &[u8],
) -> Result<[u8; HMAC_LEN], StatefsError> {
    let mut mac = mac_init(mac_key, key_path, seq, meta, payload)?;
    mac.update(payload);
    Ok(mac.finalize().into_bytes().into())
}

/// Verify-and-unwrap on the read path. `Off` passes bytes through unchanged
/// (magic is defense-in-depth only — policy decides). `Integrity` requires a
/// structurally valid envelope and a non-stale seq. `Authenticated`
/// additionally requires `alg == ALG_HMAC_SHA256` and a valid MAC.
pub fn open<'a>(
    class: PolicyClass,
    key_path: &str,
    bytes: &'a [u8],
    mac_key: Option<&EnvelopeKey>,
    tracker: &SeqTracker,
) -> Result<&'a [u8], StatefsError> {
    match class {
        PolicyClass::Off => Ok(bytes),
        PolicyClass::Integrity => {
            let env = decode(bytes)?;
            tracker.check_read(key_path, env.seq)?;
            Ok(env.payload)
        }
        PolicyClass::Authenticated => {
            let env = decode(bytes)?;
            if env.alg != ALG_HMAC_SHA256 {
                // An Integrity-class envelope where Authenticated is mandated
                // is a downgrade attempt, not a formatting error.
                return Err(StatefsError::IntegrityViolation);
            }
            let key = mac_key.ok_or(StatefsError::IntegrityViolation)?;
            env.verify_mac(key_path, key)?;
            tracker.check_read(key_path, env.seq)?;
            Ok(env.payload)
        }
    }
}

// ============================================================================
// Anti-rollback: bounded per-key max-seq tracking
// ============================================================================

/// Bounded map key -> max-seen seq, fed at replay and consulted at put/read.
#[derive(Debug, Default)]
pub struct SeqTracker {
    max_seen: BTreeMap<String, u64>,
}

impl SeqTracker {
    /// Create an empty tracker.
    pub fn new() -> Self {
        Self { max_seen: BTreeMap::new() }
    }

    /// Replay-time observation: record the max seq per key. Idempotent —
    /// replaying the same journal twice yields the same state.
    pub fn observe(&mut self, key: &str, seq: u64) -> Result<(), StatefsError> {
        match self.max_seen.get_mut(key) {
            Some(max) => {
                if seq > *max {
                    *max = seq;
                }
                Ok(())
            }
            None => {
                if self.max_seen.len() >= MAX_TRACKED_KEYS {
                    return Err(StatefsError::ReplayLimitExceeded);
                }
                self.max_seen.insert(String::from(key), seq);
                Ok(())
            }
        }
    }

    /// Put-path check: a seq <= max-seen for the key is a rollback; a fresh
    /// seq advances the high-water mark.
    pub fn check_put(&mut self, key: &str, seq: u64) -> Result<(), StatefsError> {
        if let Some(&max) = self.max_seen.get(key) {
            if seq <= max {
                return Err(StatefsError::RollbackDetected);
            }
        } else if self.max_seen.len() >= MAX_TRACKED_KEYS {
            return Err(StatefsError::ReplayLimitExceeded);
        }
        self.max_seen.insert(String::from(key), seq);
        Ok(())
    }

    /// Read-path check: a seq strictly below max-seen means the stored value
    /// was rolled back (equal is the expected steady state).
    pub fn check_read(&self, key: &str, seq: u64) -> Result<(), StatefsError> {
        if let Some(&max) = self.max_seen.get(key) {
            if seq < max {
                return Err(StatefsError::RollbackDetected);
            }
        }
        Ok(())
    }

    /// Max-seen seq for a key, if tracked.
    pub fn max_seen(&self, key: &str) -> Option<u64> {
        self.max_seen.get(key).copied()
    }

    /// Number of tracked keys.
    pub fn len(&self) -> usize {
        self.max_seen.len()
    }

    /// True when no keys are tracked.
    pub fn is_empty(&self) -> bool {
        self.max_seen.is_empty()
    }
}

// ============================================================================
// Latency budget hook
// ============================================================================

/// Deterministic write-latency budget: the caller injects elapsed time, so
/// tests never depend on a wall clock.
#[derive(Debug, Clone, Copy)]
pub struct WriteBudget {
    budget_ns: u64,
    warn_count: u64,
}

impl WriteBudget {
    /// Create a budget of `budget_ns` nanoseconds per write.
    pub fn new(budget_ns: u64) -> Self {
        Self { budget_ns, warn_count: 0 }
    }

    /// Record one operation; returns true (and counts a warning) when
    /// `elapsed_ns` strictly exceeds the budget.
    pub fn record(&mut self, elapsed_ns: u64) -> bool {
        if elapsed_ns > self.budget_ns {
            self.warn_count = self.warn_count.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Number of budget-exceeded warnings so far.
    pub fn warn_count(&self) -> u64 {
        self.warn_count
    }

    /// Configured budget in nanoseconds.
    pub fn budget_ns(&self) -> u64 {
        self.budget_ns
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    //! Only the journal-roundtrip test lives here (it needs the crate-private
    //! device handle); the public-API contract tests are in `tests/envelope.rs`.
    use super::*;
    use crate::JournalEngine;
    use storage::MemBlockDevice;

    const KEY: &str = "/state/settingsd/prefs";
    const PAYLOAD: &[u8] = b"theme=dark\nscale=2\n";

    #[test]
    fn test_wrap_put_replay_verify_same_bytes() {
        // Envelope survives the journal round-trip byte-identically
        // (value-internal format; RFC-0018 record layout untouched).
        let key = EnvelopeKey([0x42; 32]);
        let meta = EnvelopeMeta { subject: "settingsd", purpose: "prefs", ts: 1_723_600_000 };
        let sealed = seal(PolicyClass::Authenticated, KEY, 11, meta, PAYLOAD, Some(&key)).unwrap();
        let device = MemBlockDevice::new(512, 100);
        let mut engine = JournalEngine::open(device).unwrap();
        engine.put(KEY, &sealed).unwrap();
        engine.sync().unwrap();
        let engine2 = JournalEngine::open(engine.device).unwrap();
        let replayed = engine2.get(KEY).unwrap();
        assert_eq!(replayed, sealed);
        let mut tracker = SeqTracker::new();
        tracker.observe(KEY, decode(&replayed).unwrap().seq).unwrap();
        let out = open(PolicyClass::Authenticated, KEY, &replayed, Some(&key), &tracker).unwrap();
        assert_eq!(out, PAYLOAD);
    }
}
