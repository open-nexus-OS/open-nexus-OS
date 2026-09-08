// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS flusher for [`crate::deny_tally::DenyTally`] (TASK-0043 P4):
//! a lazily routed metricsd client (ONE bounded routing attempt — never
//! retried once it failed, so an unreachable metricsd costs nothing after
//! the first deny) and a rate-limited flush. Counting never fails the
//! enforcer. Split out of `lib.rs` for the structure ratchet.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: accounting in deny_tally.rs (host); OS via policyd/statefsd
use crate::client::MetricsClient;
use crate::deny_tally::{subject_label, DenyTally};

/// One counter's tally + transport state.
pub struct DenyCounter {
    name: &'static str,
    tally: DenyTally,
    client: Option<MetricsClient>,
    routing_failed: bool,
}

impl DenyCounter {
    /// `name` is the counter (`quota_denies_total`, …).
    pub const fn new(name: &'static str) -> Self {
        Self { name, tally: DenyTally::new(), client: None, routing_failed: false }
    }

    /// Counts one refusal and flushes when due (bounded IPC).
    pub fn note(&mut self, subject: u64, now_ns: u64) {
        self.tally.note(subject);
        self.flush_if_due(now_ns);
    }

    /// Refusals counted since boot.
    pub fn total(&self) -> u64 {
        self.tally.total()
    }

    fn flush_if_due(&mut self, now_ns: u64) {
        if !self.tally.flush_due(now_ns) {
            return;
        }
        if self.client.is_none() {
            if self.routing_failed {
                return;
            }
            match MetricsClient::new() {
                Ok(c) => self.client = Some(c),
                Err(_) => {
                    self.routing_failed = true;
                    return;
                }
            }
        }
        let name = self.name;
        let client = self.client.as_ref();
        self.tally.drain(now_ns, |subject, delta| {
            if let Some(c) = client {
                let mut label = [0u8; 32];
                let n = subject_label(subject, &mut label);
                let _ = c.counter_inc(name, &label[..n], delta);
            }
        });
    }
}
