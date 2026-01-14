// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: Bounded in-memory journal (ring buffer) for structured log records
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0017-service-architecture.md

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

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

/// Stored log record (owned bytes, bounded by upstream validation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    pub record_id: RecordId,
    pub timestamp_nsec: TimestampNsec,
    pub level: LogLevel,
    pub service_id: u64,
    pub scope: Vec<u8>,
    pub message: Vec<u8>,
    pub fields: Vec<u8>,
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
    next_id: u64,
    total_records: u64,
    dropped_records: u64,
    used_bytes: u32,
    records: VecDeque<LogRecord>,
}

impl Journal {
    /// Creates a new journal with fixed capacities.
    pub fn new(cap_records: u32, cap_bytes: u32) -> Self {
        Self {
            cap_records: cap_records.max(1),
            cap_bytes: cap_bytes.max(1),
            next_id: 1,
            total_records: 0,
            dropped_records: 0,
            used_bytes: 0,
            records: VecDeque::new(),
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
        let size = record_size(scope.len(), message.len(), fields.len())?;
        if size > self.cap_bytes {
            return Err(JournalError::TooLarge);
        }

        // Ensure capacity for the new record.
        while (self.records.len() as u32) >= self.cap_records || self.used_bytes.saturating_add(size) > self.cap_bytes
        {
            if let Some(old) = self.records.pop_front() {
                self.used_bytes = self.used_bytes.saturating_sub(old.size_bytes);
                self.dropped_records = self.dropped_records.saturating_add(1);
            } else {
                break;
            }
        }

        // If we still can't fit, reject deterministically.
        if (self.records.len() as u32) >= self.cap_records || self.used_bytes.saturating_add(size) > self.cap_bytes {
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
            scope: scope.to_vec(),
            message: message.to_vec(),
            fields: fields.to_vec(),
            size_bytes: size,
        };
        self.records.push_back(rec);
        self.used_bytes = self.used_bytes.saturating_add(size);

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
