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
