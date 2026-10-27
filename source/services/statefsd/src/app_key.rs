// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the app key class of statefs (TASK-0333 6e-1, RFC-0100 §6, ADR-0086):
//! `/state/app/<owner>/…` is ONE app's own record. The owner is the sender's kernel identity
//! (`sender_service_id`, the launcher-derived `app:<bundle>` sid — stable across launches,
//! never a payload string): an app writes `/state/app/@self/…` and the token is rewritten to
//! its own identity before any other rule runs; a key that names another owner is nobody's
//! business but a system service's (the generic `statefs.*` grants through policyd, as before).
//! The route itself (`svc.state`, granted by `restorable = true`) is the capability — no
//! policyd ask per request, no app grant in the policy file. Pure, host-tested.
//! OWNERS: @storage
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (`test_reject_app_key_of_another_owner`, the rewrite)

use alloc::string::String;

/// The app key class prefix.
pub(crate) const APP_PREFIX: &str = "/state/app/";
/// The token an app writes for its own identity.
pub(crate) const SELF_TOKEN: &str = "@self";

/// How an app key resolves for `sender`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppKey {
    /// Not an app key: the generic rules decide.
    NotApp,
    /// The sender's own record (`@self` rewritten, or its identity named): allowed as such.
    Own(String),
    /// Another owner's record: only a holder of the generic grant may touch it.
    Foreign,
}

/// Resolves `key` for `sender` (sid 0 — a nameless sender — owns nothing).
#[must_use]
pub(crate) fn resolve(key: &str, sender: u64) -> AppKey {
    let Some(rest) = key.strip_prefix(APP_PREFIX) else { return AppKey::NotApp };
    let (owner, tail) = rest.split_once('/').unwrap_or((rest, ""));
    if sender == 0 {
        return AppKey::Foreign;
    }
    let own = alloc::format!("{sender:016x}");
    if owner == SELF_TOKEN {
        let mut out = String::with_capacity(APP_PREFIX.len() + own.len() + 1 + tail.len());
        out.push_str(APP_PREFIX);
        out.push_str(&own);
        if !tail.is_empty() {
            out.push('/');
            out.push_str(tail);
        }
        return AppKey::Own(out);
    }
    if owner == own {
        return AppKey::Own(String::from(key));
    }
    AppKey::Foreign
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_rewrites_to_the_senders_identity_and_a_named_own_key_stays() {
        assert_eq!(
            resolve("/state/app/@self/persist", 0x1234),
            AppKey::Own(String::from("/state/app/0000000000001234/persist"))
        );
        assert_eq!(
            resolve("/state/app/0000000000001234/persist", 0x1234),
            AppKey::Own(String::from("/state/app/0000000000001234/persist"))
        );
        assert_eq!(
            resolve("/state/app/@self", 0x1234),
            AppKey::Own(String::from("/state/app/0000000000001234"))
        );
        assert_eq!(resolve("/state/init/restarts", 0x1234), AppKey::NotApp);
        assert_eq!(resolve("/state/apple", 0x1234), AppKey::NotApp);
    }

    #[test]
    fn test_reject_app_key_of_another_owner() {
        // Another app's record, by identity: foreign — the generic grant decides, which no app holds.
        assert_eq!(resolve("/state/app/0000000000001234/persist", 0x5678), AppKey::Foreign);
        // A spoofed token is not a token.
        assert_eq!(resolve("/state/app/@Self/persist", 0x1234), AppKey::Foreign);
        assert_eq!(resolve("/state/app/@self2/persist", 0x1234), AppKey::Foreign);
        // A nameless sender owns nothing, not even `@self`.
        assert_eq!(resolve("/state/app/@self/persist", 0), AppKey::Foreign);
        // A hex that is not the sender's sixteen digits (short form) is foreign.
        assert_eq!(resolve("/state/app/1234/persist", 0x1234), AppKey::Foreign);
    }
}
