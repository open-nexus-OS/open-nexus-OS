// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0072 amendment (TASK-0043 P1) — quota enforcement at the
//! statefsd `put` seam, after the capability check and the RFC-0091
//! argument evaluation, before the envelope check and the journal append.
//! The build-time `QUOTA_ENTRIES` (policies SSOT) name the prefix sets;
//! `used` is recomputed from the engine's replayed map on every metered
//! put (`used_under`), so no counter can drift across reopen, the virtio
//! upgrade or compaction. Hard ⇒ `STATUS_QUOTA_EXCEEDED` + `statefs: quota
//! deny …` (audited); soft ⇒ `statefs: quota warn …` once per boot per
//! rule (`WarnLatch`). `del` is never quota-denied.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: model in tests/state_quota_host/; QEMU `statefs: quota
//!   deny …` + `SELFTEST: quota deny ok`
//! RFC: docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md

use statefs::quota::{check_put, rule_for, PutVerdict, QuotaRule, WarnLatch};
use statefs::JournalEngine;
use storage::BlockDevice;

use crate::emit_os::{emit_quota_deny, emit_quota_warn};

mod quota_table {
    include!(concat!(env!("OUT_DIR"), "/quota_table.rs"));
}

/// Per-boot quota state (the warn latches + the deny counter; usage is never cached).
pub(crate) struct QuotaState {
    latch: WarnLatch,
    /// `quota_denies_total{subject}` (TASK-0043 P4), flushed to metricsd at
    /// most once per second — bounded, best-effort.
    denies: nexus_metrics::deny_counter::DenyCounter,
}

impl QuotaState {
    pub(crate) const fn new() -> Self {
        Self {
            latch: WarnLatch::new(),
            denies: nexus_metrics::deny_counter::DenyCounter::new("quota_denies_total"),
        }
    }

    /// The declared rules (build-time table).
    pub(crate) fn rules() -> &'static [QuotaRule] {
        quota_table::QUOTA_ENTRIES
    }

    /// Quota gate for `put(key, value)`: `true` = proceed. On a hard-limit
    /// refusal the deny marker/audit is emitted here; a soft-limit crossing
    /// is announced once (the put still proceeds).
    pub(crate) fn admit_put<B: BlockDevice>(
        &mut self,
        engine: &JournalEngine<B>,
        key: &str,
        new_len: usize,
    ) -> bool {
        let rules = Self::rules();
        let Some(idx) = rule_for(rules, key) else { return true };
        let rule = &rules[idx];
        let used = engine.used_under(rule.prefixes);
        match check_put(rule, used, key.len(), engine.stored_len(key), new_len) {
            PutVerdict::Ok => {
                let _ = self.latch.observe(idx, used, rule.soft_bytes);
                true
            }
            PutVerdict::Warn { used: next } => {
                if self.latch.observe(idx, next, rule.soft_bytes) {
                    emit_quota_warn(rule.subject, next, rule.soft_bytes);
                }
                true
            }
            PutVerdict::Deny { used } => {
                emit_quota_deny(rule.subject, used, rule.hard_bytes);
                self.denies.note(rule.subject, nexus_abi::nsec().unwrap_or(0));
                false
            }
        }
    }

    /// Re-arms the soft warning when a delete brings usage back under soft.
    pub(crate) fn note_delete<B: BlockDevice>(&mut self, engine: &JournalEngine<B>, key: &str) {
        let rules = Self::rules();
        if let Some(idx) = rule_for(rules, key) {
            let rule = &rules[idx];
            let _ = self.latch.observe(idx, engine.used_under(rule.prefixes), rule.soft_bytes);
        }
    }
}
