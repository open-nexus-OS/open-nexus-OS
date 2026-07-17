// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: Bounded in-memory journal (ring buffer) for structured log records
//!
//! OWNERS: @runtime
//!
//! STATUS: Experimental
//!
//! API_STABILITY: Unstable
//!
//! TEST_COVERAGE: Tests in `source/services/logd/tests/journal_protocol.rs`
//!   - Drop-oldest by records/bytes, query edge cases, stats, capacity limits
//!   - Property tests for panic-freedom
//!
//! ADR: docs/adr/0017-service-architecture.md

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

// Storage bounds (kept smaller than the wire protocol maxima to control heap footprint in
// os-lite builds). These must be large enough to satisfy selftests (crash report + probes).
pub const STORE_MAX_SCOPE_LEN: usize = 32;
pub const STORE_MAX_MSG_LEN: usize = 128;
pub const STORE_MAX_FIELDS_LEN: usize = 128;

/// Monotonic record id (per-journal).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct RecordId(pub u64);

/// Timestamp in nanoseconds since boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TimestampNsec(pub u64);

/// Log level (v1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Fixed-size byte storage to avoid per-record heap allocations (bump allocator friendly).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineBytes<const N: usize> {
    len: u16,
    buf: [u8; N],
}

impl<const N: usize> InlineBytes<N> {
    pub fn new(src: &[u8]) -> Self {
        let mut buf = [0u8; N];
        let n = core::cmp::min(src.len(), N);
        buf[..n].copy_from_slice(&src[..n]);
        Self { len: n as u16, buf }
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..(self.len as usize)]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Stored log record (owned bytes, bounded by upstream validation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    pub record_id: RecordId,
    pub timestamp_nsec: TimestampNsec,
    pub level: LogLevel,
    pub service_id: u64,
    pub scope: InlineBytes<STORE_MAX_SCOPE_LEN>,
    pub message: InlineBytes<STORE_MAX_MSG_LEN>,
    pub fields: InlineBytes<STORE_MAX_FIELDS_LEN>,
    pub(crate) size_bytes: u32,
}

/// Append outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppendOutcome {
    pub record_id: RecordId,
    pub dropped_records: u64,
}

/// Journal stats snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalStats {
    pub total_records: u64,
    pub dropped_records: u64,
    pub capacity_records: u32,
    pub capacity_bytes: u32,
    pub used_records: u32,
    pub used_bytes: u32,
}

/// Errors returned by journal operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "journal errors must be handled"]
pub enum JournalError {
    /// The record cannot fit into the journal under the configured bounds.
    TooLarge,
}

/// Bounded RAM journal: fixed caps; overflow drops oldest.
pub struct Journal {
    cap_records: u32,
    cap_bytes: u32,
    alloc_cap_bytes: u32,
    // Lifetime allocated bytes budget (for bump allocators). NOTE: dropping records does not
    // reclaim heap in bump allocators, so this is intentionally monotonic.
    total_allocated_bytes: u32,
    next_id: u64,
    total_records: u64,
    dropped_records: u64,
    used_bytes: u32,
    records: VecDeque<LogRecord>,
}

impl Journal {
    /// Creates a new journal with fixed capacities.
    pub fn new(cap_records: u32, cap_bytes: u32) -> Self {
        Self::new_with_alloc_cap(cap_records, cap_bytes, u32::MAX)
    }

    /// Creates a journal with an explicit allocation cap (for bump allocators).
    pub fn new_with_alloc_cap(cap_records: u32, cap_bytes: u32, alloc_cap_bytes: u32) -> Self {
        let cap_records = cap_records.max(1);
        Self {
            cap_records,
            cap_bytes: cap_bytes.max(1),
            alloc_cap_bytes: alloc_cap_bytes.max(1),
            total_allocated_bytes: 0,
            next_id: 1,
            total_records: 0,
            dropped_records: 0,
            used_bytes: 0,
            // Pre-allocate to avoid growth patterns that leak heap under bump allocators.
            records: VecDeque::with_capacity(cap_records as usize),
        }
    }

    /// Appends a record, dropping oldest entries as needed.
    ///
    /// The caller must ensure per-field bounds (scope/message/fields) before calling.
    pub fn append(
        &mut self,
        service_id: u64,
        timestamp_nsec: TimestampNsec,
        level: LogLevel,
        scope: &[u8],
        message: &[u8],
        fields: &[u8],
    ) -> Result<AppendOutcome, JournalError> {
        let scope_len = core::cmp::min(scope.len(), STORE_MAX_SCOPE_LEN);
        let msg_len = core::cmp::min(message.len(), STORE_MAX_MSG_LEN);
        let fields_len = core::cmp::min(fields.len(), STORE_MAX_FIELDS_LEN);
        let size = record_size(scope_len, msg_len, fields_len)?;
        if size > self.cap_bytes {
            return Err(JournalError::TooLarge);
        }
        // Allocation cap is a lifetime budget (bump allocator friendly): if this is exceeded,
        // the journal rejects new records rather than exhausting the process heap.
        if self.total_allocated_bytes.saturating_add(size) > self.alloc_cap_bytes {
            return Err(JournalError::TooLarge);
        }

        // Ensure capacity for the new record.
        while (self.records.len() as u32) >= self.cap_records
            || self.used_bytes.saturating_add(size) > self.cap_bytes
        {
            if let Some(old) = self.records.pop_front() {
                self.used_bytes = self.used_bytes.saturating_sub(old.size_bytes);
                self.dropped_records = self.dropped_records.saturating_add(1);
            } else {
                break;
            }
        }

        // If we still can't fit, reject deterministically.
        if (self.records.len() as u32) >= self.cap_records
            || self.used_bytes.saturating_add(size) > self.cap_bytes
        {
            return Err(JournalError::TooLarge);
        }
        // Allocation cap is a secondary guardrail (primarily for constrained allocators).
        // Enforce it on *current* usage, not lifetime allocated bytes, so drop-oldest rotation
        // doesn't permanently disable logging under steady-state workloads.
        if self.used_bytes.saturating_add(size) > self.alloc_cap_bytes {
            return Err(JournalError::TooLarge);
        }

        let record_id = RecordId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.total_records = self.total_records.saturating_add(1);

        let rec = LogRecord {
            record_id,
            timestamp_nsec,
            level,
            service_id,
            scope: InlineBytes::new(&scope[..scope_len]),
            message: InlineBytes::new(&message[..msg_len]),
            fields: InlineBytes::new(&fields[..fields_len]),
            size_bytes: size,
        };
        self.records.push_back(rec);
        self.used_bytes = self.used_bytes.saturating_add(size);
        self.total_allocated_bytes = self.total_allocated_bytes.saturating_add(size);

        Ok(AppendOutcome { record_id, dropped_records: self.dropped_records })
    }

    /// Returns up to `max_count` records with timestamp >= `since_nsec`.
    ///
    /// This is O(n) and bounded by journal size (v1).
    pub fn query(&self, since_nsec: TimestampNsec, max_count: u16) -> Vec<LogRecord> {
        let mut out = Vec::new();
        let cap = core::cmp::min(max_count as usize, self.records.len());
        if cap == 0 {
            return out;
        }
        out.reserve(cap);
        for rec in self.records.iter() {
            if rec.timestamp_nsec.0 >= since_nsec.0 {
                out.push(rec.clone());
                if out.len() >= cap {
                    break;
                }
            }
        }
        out
    }

    /// Iterates records with timestamp >= `since_nsec`, in insertion order.
    ///
    /// This is allocation-free and is preferred for OS-lite query encoding under bump allocators.
    pub fn iter_since(&self, since_nsec: TimestampNsec) -> impl Iterator<Item = &LogRecord> {
        self.records.iter().filter(move |rec| rec.timestamp_nsec.0 >= since_nsec.0)
    }

    /// Returns current journal stats.
    pub fn stats(&self) -> JournalStats {
        JournalStats {
            total_records: self.total_records,
            dropped_records: self.dropped_records,
            capacity_records: self.cap_records,
            capacity_bytes: self.cap_bytes,
            used_records: self.records.len() as u32,
            used_bytes: self.used_bytes,
        }
    }

    /// Fold every stored record into ONE verdict per subject (`scope`) — RFC-0068 §4: logd is the
    /// central subject-indexed journal, so records about one subject emitted by DIFFERENT processes
    /// (e.g. init wiring policyd + policyd's own audits) collect into a single `scope` verdict here,
    /// the cross-process grouping a per-process counter cannot do. A record at Error/Warn level is a
    /// failure (loud in the verdict); Info/Debug/Trace pass. Each subject's span runs from its
    /// earliest record to `flush_nsec`. Subjects are returned in first-seen order. The verdict math
    /// is the shared `nexus-event` SSOT (same OK/WARN-slow/ERROR the console grid uses).
    pub fn subject_verdicts(&self, flush_nsec: TimestampNsec) -> Vec<SubjectVerdict> {
        // (scope, total, fails, first_ns) accumulators in first-seen order — small N (subjects),
        // linear find is fine and keeps the order stable for a deterministic render.
        let mut acc: Vec<(InlineBytes<STORE_MAX_SCOPE_LEN>, u32, u32, u64)> = Vec::new();
        for rec in &self.records {
            let failed = matches!(rec.level, LogLevel::Error | LogLevel::Warn);
            let ts = rec.timestamp_nsec.0;
            if let Some(entry) = acc.iter_mut().find(|(s, ..)| s.as_slice() == rec.scope.as_slice())
            {
                entry.1 = entry.1.saturating_add(1);
                if failed {
                    entry.2 = entry.2.saturating_add(1);
                }
                if ts < entry.3 {
                    entry.3 = ts;
                }
            } else {
                acc.push((rec.scope, 1, u32::from(failed), ts));
            }
        }
        acc.into_iter()
            .map(|(scope, total, fails, first)| SubjectVerdict {
                scope,
                verdict: nexus_event::verdict_from(total, fails, Some(first), flush_nsec.0),
            })
            .collect()
    }
}

/// One subject's folded verdict over the journal (the row a subject-grouped renderer prints).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubjectVerdict {
    /// The subject this verdict is about (the records' `scope`).
    pub scope: InlineBytes<STORE_MAX_SCOPE_LEN>,
    /// The folded verdict (passed/total, ms, OK/WARN-slow/ERROR) via the shared SSOT math.
    pub verdict: nexus_event::Verdict,
}

fn record_size(scope_len: usize, msg_len: usize, fields_len: usize) -> Result<u32, JournalError> {
    // Conservative accounting: payload bytes + fixed header overhead.
    const OVERHEAD: u32 = 64;
    let sum = (scope_len as u32)
        .saturating_add(msg_len as u32)
        .saturating_add(fields_len as u32)
        .saturating_add(OVERHEAD);
    if sum == 0 {
        return Err(JournalError::TooLarge);
    }
    Ok(sum)
}

#[cfg(test)]
mod subject_verdict_tests {
    use super::*;

    #[test]
    fn folds_records_by_scope_across_emitters() {
        // RFC-0068 §4 proof: records about `policyd` from DIFFERENT emitters (init wiring it +
        // policyd's own) fold into ONE subject verdict at the central journal.
        let mut j = Journal::new(16, 64 * 1024);
        j.append(1, TimestampNsec(10_000_000), LogLevel::Info, b"policyd", b"grant", b"").unwrap();
        j.append(1, TimestampNsec(11_000_000), LogLevel::Info, b"policyd", b"wire", b"").unwrap();
        // different emitter (service_id 7), same subject scope:
        j.append(7, TimestampNsec(40_000_000), LogLevel::Info, b"policyd", b"ready", b"").unwrap();
        // a distinct subject, at Error level (a failure):
        j.append(9, TimestampNsec(20_000_000), LogLevel::Error, b"gpud", b"fault", b"").unwrap();

        let sv = j.subject_verdicts(TimestampNsec(50_000_000));
        assert_eq!(sv.len(), 2, "two subjects, not four emitter rows");

        let policyd = sv.iter().find(|s| s.scope.as_slice() == b"policyd").unwrap();
        assert_eq!(policyd.verdict.total, 3); // 2 init + 1 policyd merged
        assert_eq!(policyd.verdict.passed, 3);
        assert_eq!(policyd.verdict.tag, nexus_event::VerdictTag::Ok);
        assert_eq!(policyd.verdict.ms, 40); // earliest 10ms → flush 50ms

        let gpud = sv.iter().find(|s| s.scope.as_slice() == b"gpud").unwrap();
        assert_eq!(gpud.verdict.total, 1);
        assert_eq!(gpud.verdict.tag, nexus_event::VerdictTag::Error); // Error level counts as fail
    }

    #[test]
    fn empty_journal_has_no_subjects() {
        let j = Journal::new(8, 8 * 1024);
        assert!(j.subject_verdicts(TimestampNsec(1000)).is_empty());
    }

    #[test]
    fn renders_subject_grid_from_journal_end_to_end() {
        // The full logd subject-grid pipeline: journal records → subject_verdicts → the shared
        // render_verdict_line → one grid line per subject, identical in format to the console grid.
        let mut j = Journal::new(16, 64 * 1024);
        j.append(1, TimestampNsec(5_000_000), LogLevel::Info, b"gpud", b"ready", b"").unwrap();
        j.append(1, TimestampNsec(6_000_000), LogLevel::Info, b"gpud", b"draw", b"").unwrap();
        let sv = j.subject_verdicts(TimestampNsec(11_000_000));
        let g = sv.iter().find(|s| s.scope.as_slice() == b"gpud").unwrap();
        let subj = core::str::from_utf8(g.scope.as_slice()).unwrap();
        let mut buf = [0u8; 96];
        let n = nexus_event::render_verdict_line(&mut buf, 11_000_000, subj, g.verdict);
        let line = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(line.contains("OK"), "{line}");
        assert!(line.contains("gpud"), "{line}");
        assert!(line.contains("2/2"), "{line}");
        assert!(line.ends_with('\n'));
    }
}
