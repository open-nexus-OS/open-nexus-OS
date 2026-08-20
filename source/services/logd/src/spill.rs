// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: Pure spill engine for the persistent evidence journal
//! (TASK-0049C, RFC-0087 §5). Evidence-class records are persisted to a
//! fixed slot ring under `/state/logd/evidence/` — drop-oldest IS the ring
//! rotation, and the byte budget holds by construction
//! (`MAX_SLOTS × SLOT_VALUE_CAP`), never by a GC walking the store
//! (ADR-0043: `/state` is a KV of small records). A monotone spill sequence
//! (`head` key) orders records across boots and tells a fresh boot how many
//! slots are live. The engine is pure (no IPC/clock): it plans key/value
//! bytes; the OS layer rides them on ONE journal-v2 transaction
//! (slot PUT + head PUT, both-or-neither — proven 2PC semantics from
//! TASK-0026, no new durability code). Values persist the STORE
//! representation of a record (scope ≤ 32, message ≤ 128, fields ≤ 128 —
//! `journal.rs` InlineBytes caps): RAM queries and post-reboot queries tell
//! the same (bounded) story, and a slot value stays far below the cap.
//!
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/evidence_spill.rs` (layout roundtrip, ring
//!   rotation, budget bound, head ordering, decode rejects).
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§5)

extern crate alloc;

use alloc::vec::Vec;

use crate::journal::{LogLevel, LogRecord};

/// Evidence ring size: 32 slots × ≤ `SLOT_VALUE_CAP` ≈ 16 KiB on-disk cap.
pub const MAX_SLOTS: u64 = 32;
/// Upper bound for one encoded slot value (header + 32 + 128 + 128 < 512).
pub const SLOT_VALUE_CAP: usize = 512;
/// Value layout version (first byte of every slot value).
pub const VALUE_VERSION: u8 = 1;

/// Key of the spill-sequence head record (next sequence, u64le).
pub const HEAD_KEY: &str = "/state/logd/evidence/head";
/// Slot key prefix; the slot index (`seq % MAX_SLOTS`) is appended.
pub const SLOT_KEY_PREFIX: &str = "/state/logd/evidence/slot_";

/// One planned spill: the slot write + the new head, to ride ONE txn.
#[derive(Debug)]
pub struct SpillPlan {
    /// Slot key (`/state/logd/evidence/slot_<i>`), ASCII, bounded.
    pub slot_key: SlotKey,
    /// Encoded slot value (layout v1).
    pub value: Vec<u8>,
    /// Encoded head value: the sequence AFTER this record (u64le).
    pub head_value: [u8; 8],
}

/// Bounded ASCII slot key (no alloc-churn on the OS bump allocator).
#[derive(Debug)]
pub struct SlotKey {
    buf: [u8; 40],
    len: usize,
}

impl SlotKey {
    fn new(slot: u64) -> Self {
        let mut buf = [0u8; 40];
        let prefix = SLOT_KEY_PREFIX.as_bytes();
        buf[..prefix.len()].copy_from_slice(prefix);
        let mut len = prefix.len();
        // Two decimal digits suffice for MAX_SLOTS ≤ 100; fixed-width keeps
        // LIST output lexicographically stable.
        let d0 = (slot / 10) % 10;
        let d1 = slot % 10;
        buf[len] = b'0' + d0 as u8;
        buf[len + 1] = b'0' + d1 as u8;
        len += 2;
        Self { buf, len }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or(SLOT_KEY_PREFIX)
    }
}

/// Pure spill sequencer: owns the monotone sequence, plans slot writes.
#[derive(Debug)]
pub struct SpillEngine {
    /// Next spill sequence (== persisted head after the last commit).
    seq: u64,
}

impl SpillEngine {
    /// `head` = value of `HEAD_KEY` from the store (0 on a fresh store).
    pub fn new(head: u64) -> Self {
        Self { seq: head }
    }

    /// Sequence the NEXT spill will use (== live record count until the
    /// ring wraps).
    pub fn next_seq(&self) -> u64 {
        self.seq
    }

    /// Number of live slots a loader must read: `min(head, MAX_SLOTS)`.
    pub fn live_slots(head: u64) -> u64 {
        core::cmp::min(head, MAX_SLOTS)
    }

    /// Plans the spill of one evidence record and advances the sequence.
    /// The caller must only advance state on commit success — on a failed
    /// txn, call `rollback_one` so the sequence mirrors the store again.
    pub fn plan(&mut self, record: &LogRecord) -> SpillPlan {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        SpillPlan {
            slot_key: SlotKey::new(seq % MAX_SLOTS),
            value: encode_slot_value(seq, record),
            head_value: self.seq.to_le_bytes(),
        }
    }

    /// Reverts one `plan` after a failed commit (store still has old head).
    pub fn rollback_one(&mut self) {
        self.seq = self.seq.wrapping_sub(1);
    }
}

/// Encodes one record as a slot value (layout v1):
/// `[ver, level, service_id:u64le, ts:u64le, seq:u64le,
///   scope_len:u8, msg_len:u8, fields_len:u8, scope, msg, fields]`
fn encode_slot_value(seq: u64, record: &LogRecord) -> Vec<u8> {
    let scope = record.scope.as_slice();
    let msg = record.message.as_slice();
    let fields = record.fields.as_slice();
    let mut out = Vec::with_capacity(29 + scope.len() + msg.len() + fields.len());
    out.push(VALUE_VERSION);
    out.push(level_byte(record.level));
    out.extend_from_slice(&record.service_id.to_le_bytes());
    out.extend_from_slice(&record.timestamp_nsec.0.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.push(scope.len().min(255) as u8);
    out.push(msg.len().min(255) as u8);
    out.push(fields.len().min(255) as u8);
    out.extend_from_slice(scope);
    out.extend_from_slice(msg);
    out.extend_from_slice(fields);
    out
}

/// A decoded slot value ready for the persisted mirror.
#[derive(Debug, PartialEq)]
pub struct SpilledRecord {
    pub seq: u64,
    pub level: LogLevel,
    pub service_id: u64,
    pub timestamp_nsec: u64,
    pub scope: Vec<u8>,
    pub message: Vec<u8>,
    pub fields: Vec<u8>,
}

/// Decodes a slot value; `None` on any layout violation (a corrupt or
/// foreign record is skipped by the loader, never trusted).
pub fn decode_slot_value(value: &[u8]) -> Option<SpilledRecord> {
    if value.len() < 29 || value.len() > SLOT_VALUE_CAP || value[0] != VALUE_VERSION {
        return None;
    }
    let level = level_from_byte(value[1])?;
    let service_id = u64::from_le_bytes(value[2..10].try_into().ok()?);
    let timestamp_nsec = u64::from_le_bytes(value[10..18].try_into().ok()?);
    let seq = u64::from_le_bytes(value[18..26].try_into().ok()?);
    let scope_len = value[26] as usize;
    let msg_len = value[27] as usize;
    let fields_len = value[28] as usize;
    let body = &value[29..];
    if body.len() != scope_len + msg_len + fields_len {
        return None;
    }
    Some(SpilledRecord {
        seq,
        level,
        service_id,
        timestamp_nsec,
        scope: body[..scope_len].to_vec(),
        message: body[scope_len..scope_len + msg_len].to_vec(),
        fields: body[scope_len + msg_len..].to_vec(),
    })
}

fn level_byte(level: LogLevel) -> u8 {
    match level {
        LogLevel::Error => 0,
        LogLevel::Warn => 1,
        LogLevel::Info => 2,
        LogLevel::Debug => 3,
        LogLevel::Trace => 4,
    }
}

fn level_from_byte(byte: u8) -> Option<LogLevel> {
    match byte {
        0 => Some(LogLevel::Error),
        1 => Some(LogLevel::Warn),
        2 => Some(LogLevel::Info),
        3 => Some(LogLevel::Debug),
        4 => Some(LogLevel::Trace),
        _ => None,
    }
}
