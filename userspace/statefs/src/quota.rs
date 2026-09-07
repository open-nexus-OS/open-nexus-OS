// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0072 amendment (TASK-0043 P1, model TASK-0133) — the one
//! `/state` byte-quota model. A `QuotaRule` names a subject, its declared
//! prefix set and soft/hard byte limits; `used` is the sum of
//! `key_len + stored_len` over the live keys under those prefixes,
//! recomputed from the engine's replayed map (`JournalEngine::used_under`)
//! so accounting can never drift from the journal across reopen, upgrade
//! or compaction. `check_put` is the pure verdict; `WarnLatch` keeps the
//! once-per-window soft warning (re-armed when usage drops below soft).
//! `del` is never quota-denied. `core`-only, host-testable.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/state_quota_host/ (deny over hard, replay
//!   determinism, warn once, delete frees), unit tests below
//! RFC: docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md (amendment 2026-09-07)

/// Quota rules a build carries (mirrors the policy SSOT bound).
pub const MAX_QUOTA_RULES: usize = 16;
/// Prefixes per rule (RFC-0072 amendment: ≤ 8).
pub const MAX_QUOTA_PREFIXES: usize = 8;

/// One compiled `[quota."<subject>"]` declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaRule {
    /// `service_id_from_name(subject)` — for markers/audit only; attribution
    /// is by prefix set.
    pub subject: u64,
    /// Literal, canonical prefixes (1..=8).
    pub prefixes: &'static [&'static str],
    /// Warn once per window above this.
    pub soft_bytes: u64,
    /// Deny (`EDQUOTA`) above this; ≥ `soft_bytes`.
    pub hard_bytes: u64,
}

impl QuotaRule {
    /// `true` when `key` lies under one of the rule's prefixes.
    pub fn covers(&self, key: &str) -> bool {
        self.prefixes.iter().any(|p| key.starts_with(p))
    }
}

/// Bytes a live key accounts for.
pub const fn entry_bytes(key_len: usize, stored_len: usize) -> u64 {
    key_len as u64 + stored_len as u64
}

/// The rule (index) covering `key`, if any — first match; rules are
/// authored disjoint (the schema rejects overlapping prefix sets).
pub fn rule_for(rules: &[QuotaRule], key: &str) -> Option<usize> {
    rules.iter().position(|r| r.covers(key))
}

/// Verdict of a `put` against a rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutVerdict {
    /// Within soft.
    Ok,
    /// Proceeds; usage after the put is above soft.
    Warn {
        /// Usage after the put.
        used: u64,
    },
    /// Refused before the journal append (`EDQUOTA`).
    Deny {
        /// Usage before the put (unchanged).
        used: u64,
    },
}

/// `next = used − old + new`; `next > hard` ⇒ Deny, `next > soft` ⇒ Warn.
/// Saturating so a corrupt/huge accounting never wraps into an allow.
pub fn check_put(
    rule: &QuotaRule,
    used: u64,
    key_len: usize,
    old_stored_len: Option<usize>,
    new_stored_len: usize,
) -> PutVerdict {
    let old = old_stored_len.map_or(0, |l| entry_bytes(key_len, l));
    let new = entry_bytes(key_len, new_stored_len);
    let next = used.saturating_sub(old).saturating_add(new);
    if next > rule.hard_bytes {
        PutVerdict::Deny { used }
    } else if next > rule.soft_bytes {
        PutVerdict::Warn { used: next }
    } else {
        PutVerdict::Ok
    }
}

/// Once-per-window soft warning per rule (window = process lifetime,
/// re-armed when usage drops to/below soft).
#[derive(Clone, Copy, Debug, Default)]
pub struct WarnLatch {
    armed_off: [bool; MAX_QUOTA_RULES],
}

impl WarnLatch {
    /// Fresh latch (every rule armed).
    pub const fn new() -> Self {
        Self { armed_off: [false; MAX_QUOTA_RULES] }
    }

    /// Reports usage after an accepted put/del; `true` = emit the warning now.
    pub fn observe(&mut self, rule_idx: usize, used: u64, soft_bytes: u64) -> bool {
        let Some(slot) = self.armed_off.get_mut(rule_idx) else { return false };
        if used > soft_bytes {
            if *slot {
                false
            } else {
                *slot = true;
                true
            }
        } else {
            *slot = false;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULE: QuotaRule =
        QuotaRule { subject: 7, prefixes: &["/state/app/demo/"], soft_bytes: 100, hard_bytes: 200 };

    #[test]
    fn verdicts_follow_next_usage() {
        let key = "/state/app/demo/k"; // 17 bytes
        assert_eq!(check_put(&RULE, 0, key.len(), None, 10), PutVerdict::Ok);
        assert_eq!(check_put(&RULE, 0, key.len(), None, 90), PutVerdict::Warn { used: 107 });
        assert_eq!(check_put(&RULE, 0, key.len(), None, 184), PutVerdict::Deny { used: 0 });
        // Overwrite counts the delta: old 100 bytes freed first.
        assert_eq!(
            check_put(&RULE, 200, key.len(), Some(100), 100),
            PutVerdict::Warn { used: 200 }
        );
        assert_eq!(
            check_put(&RULE, 200, key.len(), Some(100), 101),
            PutVerdict::Deny { used: 200 }
        );
        // Saturation never wraps into an allow.
        assert_eq!(
            check_put(&RULE, u64::MAX, key.len(), None, 1),
            PutVerdict::Deny { used: u64::MAX }
        );
    }

    #[test]
    fn latch_warns_once_and_rearms() {
        let mut latch = WarnLatch::new();
        assert!(latch.observe(0, 150, 100));
        assert!(!latch.observe(0, 160, 100));
        assert!(!latch.observe(0, 100, 100)); // back at soft: re-armed, no warn
        assert!(latch.observe(0, 101, 100));
        assert!(!latch.observe(MAX_QUOTA_RULES, 999, 1)); // out of range: never
    }

    #[test]
    fn rule_lookup_is_by_prefix() {
        let rules = [RULE];
        assert_eq!(rule_for(&rules, "/state/app/demo/x"), Some(0));
        assert_eq!(rule_for(&rules, "/state/app/other/x"), None);
    }
}
