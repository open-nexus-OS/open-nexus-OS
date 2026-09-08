// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: bounded per-subject deny counters (TASK-0043 P4:
//! `quota_denies_total{subject}`, `egress_denies_total{subject}`,
//! `ingress_denies_total{subject}`). An enforcer tallies in-process (≤
//! `MAX_SUBJECTS` label slots — the cardinality cap under metricsd's
//! `max_series_total`; an overflow subject folds into `subject=other`) and
//! flushes the pending deltas to metricsd at most once per
//! `FLUSH_INTERVAL_NS`, so a deny storm costs one bounded IPC per second,
//! never one per refusal. Pure accounting here (host-tested); the OS flush
//! lives in `client::DenyCounter`.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below

/// Per-subject label slots per counter (≤ metricsd `MAX_SERIES_PER_METRIC`).
pub const MAX_SUBJECTS: usize = 15;
/// Flush cadence (one bounded IPC per interval per counter, at most).
pub const FLUSH_INTERVAL_NS: u64 = 1_000_000_000;
/// The overflow label when more than `MAX_SUBJECTS` subjects deny.
pub const OTHER_SUBJECT: u64 = 0;

/// Pending deltas of one counter.
#[derive(Clone, Copy, Debug)]
pub struct DenyTally {
    subjects: [u64; MAX_SUBJECTS + 1],
    pending: [u64; MAX_SUBJECTS + 1],
    used: usize,
    last_flush_ns: u64,
    /// Refusals counted since boot (all subjects).
    total: u64,
}

impl DenyTally {
    /// Empty tally (slot `MAX_SUBJECTS` is the `other` bucket).
    pub const fn new() -> Self {
        Self {
            subjects: [0; MAX_SUBJECTS + 1],
            pending: [0; MAX_SUBJECTS + 1],
            used: 0,
            last_flush_ns: 0,
            total: 0,
        }
    }

    /// Counts one refusal for `subject`.
    pub fn note(&mut self, subject: u64) {
        self.total = self.total.saturating_add(1);
        for i in 0..self.used {
            if self.subjects[i] == subject {
                self.pending[i] = self.pending[i].saturating_add(1);
                return;
            }
        }
        if self.used < MAX_SUBJECTS && subject != OTHER_SUBJECT {
            self.subjects[self.used] = subject;
            self.pending[self.used] = 1;
            self.used += 1;
        } else {
            self.pending[MAX_SUBJECTS] = self.pending[MAX_SUBJECTS].saturating_add(1);
        }
    }

    /// Refusals counted since boot.
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// `true` when a flush is due (something pending and the interval passed
    /// or never flushed).
    pub fn flush_due(&self, now_ns: u64) -> bool {
        let pending = self.pending.iter().any(|p| *p != 0);
        pending
            && (self.last_flush_ns == 0
                || now_ns.saturating_sub(self.last_flush_ns) >= FLUSH_INTERVAL_NS)
    }

    /// Hands every pending (subject, delta) to `sink` and clears them.
    /// `subject == OTHER_SUBJECT` is the overflow bucket.
    pub fn drain(&mut self, now_ns: u64, mut sink: impl FnMut(u64, u64)) {
        for i in 0..self.used {
            if self.pending[i] != 0 {
                sink(self.subjects[i], self.pending[i]);
                self.pending[i] = 0;
            }
        }
        if self.pending[MAX_SUBJECTS] != 0 {
            sink(OTHER_SUBJECT, self.pending[MAX_SUBJECTS]);
            self.pending[MAX_SUBJECTS] = 0;
        }
        self.last_flush_ns = now_ns;
    }
}

impl Default for DenyTally {
    fn default() -> Self {
        Self::new()
    }
}

/// `subject=0x<sid hex16>\n` label bytes for a counter series.
pub fn subject_label(subject: u64, out: &mut [u8; 32]) -> usize {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let head = b"subject=0x";
    out[..head.len()].copy_from_slice(head);
    let mut n = head.len();
    if subject == OTHER_SUBJECT {
        out[n..n + 5].copy_from_slice(b"other");
        n += 5;
    } else {
        for i in 0..16 {
            out[n] = HEX[((subject >> (60 - 4 * i)) & 0xf) as usize];
            n += 1;
        }
    }
    out[n] = b'\n';
    n + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tally_batches_and_caps_cardinality() {
        let mut t = DenyTally::new();
        assert!(!t.flush_due(5));
        t.note(7);
        t.note(7);
        t.note(9);
        assert!(t.flush_due(5), "first flush is immediate");
        let mut got = std::vec::Vec::new();
        t.drain(5, |s, d| got.push((s, d)));
        assert_eq!(got, std::vec![(7, 2), (9, 1)]);
        assert!(!t.flush_due(10));
        t.note(7);
        assert!(!t.flush_due(5 + FLUSH_INTERVAL_NS - 1), "rate limited");
        assert!(t.flush_due(5 + FLUSH_INTERVAL_NS));
        // Cardinality cap: subjects beyond MAX_SUBJECTS fold into `other`.
        let mut t = DenyTally::new();
        for s in 1..=(MAX_SUBJECTS as u64 + 3) {
            t.note(s);
        }
        let mut got = std::vec::Vec::new();
        t.drain(1, |s, d| got.push((s, d)));
        assert_eq!(got.len(), MAX_SUBJECTS + 1);
        assert_eq!(got[MAX_SUBJECTS], (OTHER_SUBJECT, 3));
        assert_eq!(t.total(), MAX_SUBJECTS as u64 + 3);
    }

    #[test]
    fn subject_label_is_bounded_and_stable() {
        let mut out = [0u8; 32];
        let n = subject_label(0x0102_0304_0506_0708, &mut out);
        assert_eq!(&out[..n], b"subject=0x0102030405060708\n");
        let n = subject_label(OTHER_SUBJECT, &mut out);
        assert_eq!(&out[..n], b"subject=0xother\n");
    }
}
