// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: statefsd write-path hardening v1 — per-prefix envelope policy,
//!   verify-on-put/get, replay-time anti-rollback observation, key derivation
//!   (TASK-0025; value format lives in `statefs::envelope`)
//! OWNERS: @runtime
//! STATUS: Functional (host-tested; wired into os_lite serve loop)
//! API_STABILITY: Unstable (service-internal)
//! TEST_COVERAGE: tests/hardening_contract.rs (accept/forge/rollback/
//!   oversize-meta/migration/budget + derivation determinism)
//!
//! POLICY TABLE (v1):
//!   - Authenticated-mandatory prefixes: `AUTHENTICATED_PREFIXES` (const,
//!     fail-closed — a non-envelope or Integrity-class value is refused).
//!   - Integrity floor: `PolicyClass::default_for_key` (boot-critical
//!     `/state/keystore/*` + `/state/boot/*`; chicken-egg rule — these never
//!     need the MAC key).
//!   - MIGRATION: keystored/updated still write raw (non-envelope) bytes
//!     under the Integrity floor until TASK-0025 plan step 4; such values are
//!     accepted and audited (`PutCheck::Migration`), never rejected. There is
//!     no migration window for Authenticated prefixes (fresh namespace).
//!
//! KEY DERIVATION (v1, see `statefs::derive` for the shared contract):
//!   statefsd signs `ENVELOPE_KEY_LABEL_V1` with the Ed25519 seed persisted
//!   at `/state/keystore/device.signing` (statefsd is that record's storage
//!   authority; the deterministic signature — not the seed — is the HKDF
//!   ikm). Derivation is lazy: before keystored has generated the device
//!   key, Authenticated puts fail closed with `IntegrityViolation`.
//!
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use ed25519_dalek::Signer;
use statefs::envelope::{
    self, EnvelopeKey, PolicyClass, SeqTracker, WriteBudget, ALG_HMAC_SHA256, ENVELOPE_MAGIC,
};
use statefs::StatefsError;

/// Storage path of the keystored device-key record (derivation ikm source).
pub const DEVICE_KEY_PATH: &str = "/state/keystore/device.signing";

/// Authenticated-mandatory key prefixes (v1). Fail-closed: every put under
/// these prefixes must carry a valid HMAC envelope with a fresh seq.
/// keystored/updated adoption (plan step 4) extends this table later.
pub const AUTHENTICATED_PREFIXES: &[&str] = &["/state/selftest/secure/"];

/// Prefixes whose stored values are envelope-scanned at replay (bounded).
pub const ENROLLED_PREFIXES: &[&str] =
    &["/state/keystore/", "/state/boot/", "/state/selftest/secure/"];

/// Per-write latency budget (ns) — generous for TCG + virtio-blk QD1 so the
/// warn path flags real stalls, not emulation noise.
pub const WRITE_BUDGET_NS: u64 = 250_000_000;

/// Effective policy class for a key: const Authenticated table first, then
/// the boot-critical Integrity floor from the envelope contract.
#[must_use]
pub fn policy_class_for(key: &str) -> PolicyClass {
    for prefix in AUTHENTICATED_PREFIXES {
        if key.starts_with(prefix) {
            return PolicyClass::Authenticated;
        }
    }
    PolicyClass::default_for_key(key)
}

/// Outcome of a put-path verification that did not reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutCheck {
    /// Key not enrolled; value passes through unchanged.
    Off,
    /// Enrolled (Integrity floor) but value predates envelopes — accepted
    /// during migration; the caller must audit-log it.
    Migration,
    /// Envelope decoded, verified per class, seq advanced.
    Verified,
}

/// Verify a put for `key`. Order matters: structural decode, then MAC (so a
/// forged value can never advance the seq high-water mark), then
/// `check_put` (mutates the tracker only for accepted-fresh seqs).
pub fn verify_put(
    key: &str,
    value: &[u8],
    mac_key: Option<&EnvelopeKey>,
    tracker: &mut SeqTracker,
) -> Result<PutCheck, StatefsError> {
    let class = policy_class_for(key);
    match class {
        PolicyClass::Off => Ok(PutCheck::Off),
        PolicyClass::Integrity => {
            if !has_envelope_magic(value) {
                // Documented migration case (see module header): raw legacy
                // bytes under the Integrity floor are accepted and audited.
                return Ok(PutCheck::Migration);
            }
            let env = envelope::decode(value)?;
            tracker.check_put(key, env.seq)?;
            Ok(PutCheck::Verified)
        }
        PolicyClass::Authenticated => {
            // Fail-closed: no magic, wrong alg, missing key, bad MAC and
            // stale seq are all rejections — never a migration accept.
            if !has_envelope_magic(value) {
                return Err(StatefsError::IntegrityViolation);
            }
            let env = envelope::decode(value)?;
            if env.alg != ALG_HMAC_SHA256 {
                // Integrity-class envelope where Authenticated is mandated
                // is a downgrade attempt.
                return Err(StatefsError::IntegrityViolation);
            }
            let mac_key = mac_key.ok_or(StatefsError::IntegrityViolation)?;
            env.verify_mac(key, mac_key)?;
            tracker.check_put(key, env.seq)?;
            Ok(PutCheck::Verified)
        }
    }
}

/// Verify a get for `key` against the replay-fed tracker: a stored value
/// whose seq is below the high-water mark was rolled back on the medium.
/// Migration-era raw bytes pass through (accept-and-log is the put path's
/// job; reads must keep working for keystored/updated bootstrap).
pub fn verify_get(
    key: &str,
    value: &[u8],
    mac_key: Option<&EnvelopeKey>,
    tracker: &SeqTracker,
) -> Result<(), StatefsError> {
    let class = policy_class_for(key);
    if class == PolicyClass::Off || !has_envelope_magic(value) {
        return Ok(());
    }
    envelope::open(class, key, value, mac_key, tracker).map(|_| ())
}

/// Replay-time observation outcome for one stored value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayObserve {
    /// Key not enrolled — nothing to track.
    NotEnrolled,
    /// Envelope seq recorded in the tracker.
    Observed,
    /// Enrolled key with non-envelope bytes (migration) or a malformed
    /// envelope: skipped without panic; the caller may audit it.
    Skipped,
}

/// Feed one replayed `key`/`value` into the tracker. Bounded and total:
/// malformed old values never panic and never abort the replay walk.
pub fn observe_replayed(
    key: &str,
    value: &[u8],
    tracker: &mut SeqTracker,
) -> Result<ReplayObserve, StatefsError> {
    if policy_class_for(key) == PolicyClass::Off {
        return Ok(ReplayObserve::NotEnrolled);
    }
    if !has_envelope_magic(value) {
        return Ok(ReplayObserve::Skipped);
    }
    match envelope::decode(value) {
        Ok(env) => {
            // Only the bounded-capacity error propagates (fail loud, not
            // silently untracked).
            tracker.observe(key, env.seq)?;
            Ok(ReplayObserve::Observed)
        }
        Err(_) => Ok(ReplayObserve::Skipped),
    }
}

/// Derive the v1 envelope MAC key from the persisted device-key seed:
/// deterministic Ed25519 signature over the label, then the shared HKDF
/// (`statefs::derive`). Writers obtain the identical ikm via keystored
/// `OP_DEVICE_SIGN` — the raw seed never leaves custody as a MAC key.
pub fn derive_key_from_device_seed(seed: &[u8; 32]) -> Result<EnvelopeKey, StatefsError> {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(seed);
    let signature = signing_key.sign(statefs::derive::ENVELOPE_KEY_LABEL_V1);
    statefs::derive::envelope_key_from_ikm(&signature.to_bytes())
}

/// Fresh write-budget accumulator for the serve loop.
#[must_use]
pub fn new_write_budget() -> WriteBudget {
    WriteBudget::new(WRITE_BUDGET_NS)
}

fn has_envelope_magic(value: &[u8]) -> bool {
    value.len() >= ENVELOPE_MAGIC.len() && value[..ENVELOPE_MAGIC.len()] == ENVELOPE_MAGIC
}
