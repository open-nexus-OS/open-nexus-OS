// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS envelope-key derivation v1 (TASK-0025, shared SSOT)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable derivation contract (v1) — changing it invalidates
//!   every Authenticated envelope at rest
//! TEST_COVERAGE: Host unit tests (determinism, ikm sensitivity)
//!
//! PUBLIC API:
//!   - ENVELOPE_KEY_LABEL_V1: the fixed derivation label
//!   - envelope_key_from_ikm: HKDF-SHA256(ikm) -> EnvelopeKey
//!
//! DERIVATION CONTRACT (v1):
//!   ikm  = keystored material: the deterministic Ed25519 device-key
//!          signature over `ENVELOPE_KEY_LABEL_V1` (RFC 8032 signing is
//!          deterministic, so every holder derives the same key).
//!   key  = HKDF-SHA256(salt = label, ikm, info = label), 32 bytes.
//!
//!   Two parties compute the ikm independently:
//!   - statefsd signs locally with the `/state/keystore/device.signing`
//!     record it already stores (no IPC; lazy, see statefsd::hardening).
//!   - writers ask keystored `OP_DEVICE_SIGN` over the label (label-scoped
//!     authorization via `device_sign_allowed` below — narrow
//!     `crypto.derive.statefs`, not generic signing power; the raw signing
//!     key never leaves keystored).
//!
//!   The raw device key is never used as a MAC key ("never a signing key
//!   used raw" — TASK-0025 invariant); the label gives domain separation.
//!
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use hkdf::Hkdf;
use sha2::Sha256;

use super::envelope::EnvelopeKey;
use super::StatefsError;

/// Fixed derivation label (doubles as HKDF salt and info context).
pub const ENVELOPE_KEY_LABEL_V1: &[u8] = b"statefs.envelope.v1";

/// Narrow capability that authorizes signing ONLY the derivation label via
/// keystored's device-sign op (the derivation oracle). Deliberately not
/// generic signing power — see `device_sign_allowed`.
pub const CAP_DERIVE_STATEFS: &str = "crypto.derive.statefs";

/// Full device-signing capability (keystored's general sign gate).
pub const CAP_SIGN: &str = "crypto.sign";

/// Authorization rule for a keystored device-sign request (TASK-0025):
/// the EXACT derivation label is signable under the narrow
/// `crypto.derive.statefs` cap (or full `crypto.sign`); any other payload
/// requires full `crypto.sign`. Lives here — next to the label — so the
/// oracle (keystored) and the contract can never drift; `has_cap` is the
/// injected policy check (identity from kernel IPC, never payload).
pub fn device_sign_allowed(payload: &[u8], mut has_cap: impl FnMut(&str) -> bool) -> bool {
    if payload == ENVELOPE_KEY_LABEL_V1 {
        has_cap(CAP_DERIVE_STATEFS) || has_cap(CAP_SIGN)
    } else {
        has_cap(CAP_SIGN)
    }
}

/// Derive the v1 envelope MAC key from input key material.
///
/// Fails closed (`IntegrityViolation`) instead of ever yielding a
/// degenerate key on the (structurally impossible) HKDF expand error.
pub fn envelope_key_from_ikm(ikm: &[u8]) -> Result<EnvelopeKey, StatefsError> {
    let hk = Hkdf::<Sha256>::new(Some(ENVELOPE_KEY_LABEL_V1), ikm);
    let mut okm = [0u8; 32];
    hk.expand(ENVELOPE_KEY_LABEL_V1, &mut okm).map_err(|_| StatefsError::IntegrityViolation)?;
    Ok(EnvelopeKey(okm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derivation_is_deterministic() {
        let a = envelope_key_from_ikm(b"same-ikm").unwrap();
        let b = envelope_key_from_ikm(b"same-ikm").unwrap();
        assert_eq!(a.0, b.0);
    }

    #[test]
    fn test_derivation_depends_on_ikm() {
        let a = envelope_key_from_ikm(b"ikm-a").unwrap();
        let b = envelope_key_from_ikm(b"ikm-b").unwrap();
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn test_derived_key_is_not_the_ikm() {
        // The label-scoped HKDF output must never equal the raw material.
        let ikm = [0x5au8; 32];
        let key = envelope_key_from_ikm(&ikm).unwrap();
        assert_ne!(key.0, ikm);
    }

    /// Simulated policy: the subject holds ONLY the named caps.
    fn holder_of(caps: &'static [&'static str]) -> impl FnMut(&str) -> bool {
        move |cap| caps.contains(&cap)
    }

    #[test]
    fn test_reject_non_label_sign_with_only_derive_cap() {
        // The regression the keystored deny selftest guards: a subject with
        // only the narrow derivation cap must NOT get arbitrary signatures.
        assert!(!device_sign_allowed(&[0u8; 8], holder_of(&[CAP_DERIVE_STATEFS])));
        // Near-misses of the label are "any other payload" too.
        assert!(!device_sign_allowed(b"statefs.envelope.v1x", holder_of(&[CAP_DERIVE_STATEFS])));
        assert!(!device_sign_allowed(b"statefs.envelope.v", holder_of(&[CAP_DERIVE_STATEFS])));
        assert!(!device_sign_allowed(b"", holder_of(&[CAP_DERIVE_STATEFS])));
    }

    #[test]
    fn test_reject_label_sign_without_any_cap() {
        assert!(!device_sign_allowed(ENVELOPE_KEY_LABEL_V1, holder_of(&[])));
    }

    #[test]
    fn test_label_sign_allowed_under_narrow_or_full_cap() {
        assert!(device_sign_allowed(ENVELOPE_KEY_LABEL_V1, holder_of(&[CAP_DERIVE_STATEFS])));
        assert!(device_sign_allowed(ENVELOPE_KEY_LABEL_V1, holder_of(&[CAP_SIGN])));
        // Full signing power still signs arbitrary payloads.
        assert!(device_sign_allowed(&[0u8; 8], holder_of(&[CAP_SIGN])));
    }
}
