// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Verifier-verdict policy (RFC-0089 §4, TASK-0198 Phase 1) — the
//! pure decision underneath `KeystoredVerifier`, cfg-free so the host suite
//! proves the exact mapping the OS path executes. The invariant this module
//! encodes: a verifier's "signature invalid" verdict is FINAL — the old code
//! re-adjudicated `Ok(false)` with a local in-process verify, silently
//! overriding keystored. Local fallback exists ONLY for keystored being
//! UNREACHABLE (transport failure), is loud at the call site, and still runs
//! against the same baked publisher anchor.
//! OWNERS: @security @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/updates_host/tests/verify_policy.rs
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

/// Outcome of one keystored verify round-trip, classified at the transport
/// boundary. `Protocol` means keystored ANSWERED but the exchange itself was
/// broken (bad status/payload shape) — a reachable-but-misbehaving verifier
/// must fail closed, never be second-guessed locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeystoredOutcome {
    /// Keystored answered: signature valid.
    Valid,
    /// Keystored answered: signature INVALID. Final.
    Invalid,
    /// Keystored answered, but the protocol exchange was malformed.
    Protocol(&'static str),
    /// Keystored unreachable (route/send/recv/timeout/clock).
    Unavailable(&'static str),
}

/// What the caller must do with the verification attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyDecision {
    /// Accept the signature as valid.
    Accept,
    /// Reject; stable detail label for audit/markers.
    Reject(&'static str),
    /// Run the LOCAL fallback verify (same anchor) and emit the loud
    /// `updated: verify fallback (keystored unavailable)` marker first.
    FallbackLocal(&'static str),
}

/// Maps a keystored outcome to the caller's obligation.
pub fn decide(outcome: KeystoredOutcome) -> VerifyDecision {
    match outcome {
        KeystoredOutcome::Valid => VerifyDecision::Accept,
        KeystoredOutcome::Invalid => VerifyDecision::Reject("keystored-invalid"),
        KeystoredOutcome::Protocol(detail) => VerifyDecision::Reject(detail),
        KeystoredOutcome::Unavailable(detail) => VerifyDecision::FallbackLocal(detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_keystored_says_invalid() {
        // The old code re-adjudicated Ok(false) with a local verify — the
        // verdict must be FINAL.
        assert_eq!(decide(KeystoredOutcome::Invalid), VerifyDecision::Reject("keystored-invalid"));
    }

    #[test]
    fn test_fallback_only_on_unavailable() {
        // Transport unavailability is the ONLY fallback ground...
        assert_eq!(
            decide(KeystoredOutcome::Unavailable("route")),
            VerifyDecision::FallbackLocal("route")
        );
        assert_eq!(
            decide(KeystoredOutcome::Unavailable("timeout")),
            VerifyDecision::FallbackLocal("timeout")
        );
        // ...a reachable-but-misbehaving keystored fails closed instead.
        assert_eq!(decide(KeystoredOutcome::Protocol("status")), VerifyDecision::Reject("status"));
        assert_eq!(
            decide(KeystoredOutcome::Protocol("payload")),
            VerifyDecision::Reject("payload")
        );
    }

    #[test]
    fn test_accept_only_on_valid() {
        assert_eq!(decide(KeystoredOutcome::Valid), VerifyDecision::Accept);
    }
}
