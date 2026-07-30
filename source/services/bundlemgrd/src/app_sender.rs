// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0086 app-host sender recognition — is this kernel
//! `sender_service_id` one of OUR installed apps' hosts? execd names every
//! app-host `app:<bundle_id>` via `exec_v2`, so the answer is derived from
//! the SAME registry the reply would list: no separate allowlist to drift.
//!
//! This replaces the old `sender_service_id == 0` acceptance (which let any
//! UNNAMED task read the listing — the pre-identity best effort, with the
//! follow-up recorded in `os_lite.rs`). Pure + host-tested; `os_lite.rs` is
//! RISC-V-only.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (accept/reject matrix).

/// Whether `sender_service_id` is the app-host of one of `app_ids`.
/// Deny-by-default: sid 0 (an unnamed spawn) is never an app host.
pub(crate) fn is_registered_app_host(sender_service_id: u64, app_ids: &[&str]) -> bool {
    if sender_service_id == 0 {
        return false;
    }
    app_ids.iter().any(|id| app_service_id(id) == sender_service_id)
}

/// `service_id_from_name("app:" + id)` without alloc — the same derivation
/// execd stamps at spawn (RFC-0086) and the shell's app-host uses to join the
/// window feed. FNV-1a over the prefix, then the id.
fn app_service_id(app_id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in b"app:".iter().chain(app_id.as_bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPS: [&str; 3] = ["calculator", "chat", "desktop-shell"];

    /// The platform's FNV-1a over a raw name (`nexus_abi::service_id_from_name`
    /// is OS-only; the algorithm is the contract, so mirror it in the test).
    fn sid_of(name: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in name {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    #[test]
    fn accepts_a_registered_app_host() {
        assert!(is_registered_app_host(sid_of(b"app:chat"), &APPS));
    }

    #[test]
    fn derivation_matches_the_prefixed_name() {
        for id in APPS {
            let mut name = std::vec::Vec::from(&b"app:"[..]);
            name.extend_from_slice(id.as_bytes());
            assert_eq!(app_service_id(id), sid_of(&name));
        }
    }

    #[test]
    fn test_reject_unnamed_sender() {
        // The pre-RFC-0086 hole: sid 0 was ANY unnamed task.
        assert!(!is_registered_app_host(0, &APPS));
    }

    #[test]
    fn test_reject_unregistered_and_unprefixed_senders() {
        assert!(!is_registered_app_host(sid_of(b"app:not-installed"), &APPS));
        // A BARE service name (no `app:` prefix) must not pass as an app
        // host — the prefix is what keeps bundle ids out of the service
        // namespace.
        assert!(!is_registered_app_host(sid_of(b"chat"), &APPS));
    }
}
