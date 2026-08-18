// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS journal v2 record set (2PC) on the frozen NXSF v1 framing
//! (TASK-0026; normative byte contract: docs/storage/statefs.md §Journal v2).
//! Mirrors the nxfs committed-only replay discipline (userspace/nxfs/src/
//! journal.rs): PREPARE/PAYLOAD runs become visible only after their COMMIT;
//! torn or orphaned transactions are discarded wholesale, never half-applied.
//! v1 records (Put/Delete) keep replaying byte-identically.
//! OWNERS: @runtime
//! STATUS: Functional (host-first)
//! API_STABILITY: Unstable (v2a)
//! TEST_COVERAGE: Unit tests below + tests/crash_injection.rs (SpyDevice
//! crash-injection harness, every-record-boundary both-or-neither proof)

use alloc::string::String;
use alloc::vec::Vec;

use storage::BlockDevice;

use crate::{JournalEngine, StatefsError, MAX_KEY_LEN, MAX_VALUE_SIZE, RECORD_HEADER_SIZE};

// ============================================================================
// Opcodes (v1 = frozen RFC-0018; v2 = TASK-0026 additions)
// ============================================================================

/// Journal operation codes. 0x01–0x03 are the frozen v1 set (RFC-0018);
/// 0x10–0x14 are the v2 transaction records (docs/storage/statefs.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JournalOpCode {
    Put = 0x01,
    Delete = 0x02,
    /// v1: reserved no-op. v2: snapshot boundary written by compaction;
    /// replay resets state at this record (everything before is superseded).
    Checkpoint = 0x03,
    /// Opens transaction `txn_id`. value = txn_id (u64 LE).
    TxnPrepare = 0x10,
    /// One value chunk for `key` inside a transaction.
    /// value = txn_id (u64 LE) ++ chunk bytes.
    TxnPayload = 0x11,
    /// Commits `txn_id`: all buffered payloads become visible atomically.
    TxnCommit = 0x12,
    /// Discards `txn_id` and all its buffered payloads.
    TxnAbort = 0x13,
    /// Durability barrier marker in the log. Named apart from the protocol
    /// `Sync` op (wire op 5) on purpose — this is a journal RECORD.
    SyncBarrier = 0x14,
}

impl JournalOpCode {
    pub(crate) fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::Put),
            0x02 => Some(Self::Delete),
            0x03 => Some(Self::Checkpoint),
            0x10 => Some(Self::TxnPrepare),
            0x11 => Some(Self::TxnPayload),
            0x12 => Some(Self::TxnCommit),
            0x13 => Some(Self::TxnAbort),
            0x14 => Some(Self::SyncBarrier),
            _ => None,
        }
    }
}

// ============================================================================
// Bounds (everything capped — journal bytes are untrusted input)
// ============================================================================

/// Maximum payload chunk per `TxnPayload` record (matches the ~8 KiB
/// effective IPC value ceiling; larger values arrive as multiple chunks).
pub const MAX_TXN_CHUNK: usize = 8192;
/// Maximum concurrently open (prepared, uncommitted) transactions.
pub const MAX_OPEN_TXNS: usize = 8;
/// Maximum distinct keys buffered per transaction.
pub const MAX_TXN_KEYS: usize = 32;
/// Maximum buffered bytes per transaction (keys + assembled values).
pub const MAX_TXN_BYTES: usize = 128 * 1024;
/// Maximum buffered bytes across all open transactions.
pub const MAX_TXN_TOTAL_BYTES: usize = 256 * 1024;

// ============================================================================
// Record encode / parse (NXSF framing, byte-compatible with v1)
// ============================================================================

/// A parsed journal record.
#[derive(Debug, Clone)]
pub(crate) struct JournalRecord {
    pub(crate) op: JournalOpCode,
    pub(crate) key: String,
    pub(crate) value: Vec<u8>,
}

/// Record serialization lives in `crate::record` (append-in-place encoder
/// for bump-allocator hot paths); the txn/checkpoint encode helpers moved
/// there too. Re-exported here for the existing users.
pub use crate::record::{
    encode_abort, encode_checkpoint, encode_commit, encode_payload, encode_prepare, encode_record,
    encode_record_into, encode_sync_barrier,
};

/// Per-opcode payload shape check (valid CRC does not make a record
/// well-formed). A violation is `Corrupted`: replay stops deterministically.
fn validate_shape(op: JournalOpCode, key_len: usize, value_len: usize) -> Result<(), StatefsError> {
    let ok = match op {
        // v1 shapes stay exactly as v1 replay accepted them.
        JournalOpCode::Put | JournalOpCode::Delete => true,
        // v1 journals never contain Checkpoint; the v2 shape is strict.
        JournalOpCode::Checkpoint => key_len == 0 && value_len == 8,
        JournalOpCode::TxnPrepare | JournalOpCode::TxnCommit | JournalOpCode::TxnAbort => {
            key_len == 0 && value_len == 8
        }
        JournalOpCode::TxnPayload => {
            (1..=MAX_KEY_LEN).contains(&key_len) && (8..=8 + MAX_TXN_CHUNK).contains(&value_len)
        }
        JournalOpCode::SyncBarrier => key_len == 0 && value_len == 0,
    };
    if ok {
        Ok(())
    } else {
        Err(StatefsError::Corrupted)
    }
}

/// Try to parse a journal record from a byte slice.
/// `Ok(None)` = end of journal (magic mismatch / truncated tail);
/// `Err(Corrupted)` = structurally invalid record (replay stops).
pub(crate) fn parse_record(data: &[u8]) -> Result<Option<(JournalRecord, usize)>, StatefsError> {
    if data.len() < RECORD_HEADER_SIZE {
        return Ok(None);
    }
    if u32::from_le_bytes([data[0], data[1], data[2], data[3]]) != crate::JOURNAL_MAGIC {
        return Ok(None);
    }
    let op = JournalOpCode::from_u8(data[4]).ok_or(StatefsError::Corrupted)?;
    let key_len = u16::from_le_bytes([data[5], data[6]]) as usize;
    let value_len = u32::from_le_bytes([data[7], data[8], data[9], data[10]]) as usize;
    if key_len > MAX_KEY_LEN || value_len > MAX_VALUE_SIZE {
        return Err(StatefsError::Corrupted);
    }
    validate_shape(op, key_len, value_len)?;
    let total_len = RECORD_HEADER_SIZE + key_len + value_len;
    if data.len() < total_len {
        // Truncated (torn) tail: end of journal.
        return Ok(None);
    }
    let key_end = 11 + key_len;
    let value_end = key_end + value_len;
    let value = &data[key_end..value_end];
    let stored_crc = u32::from_le_bytes([
        data[value_end],
        data[value_end + 1],
        data[value_end + 2],
        data[value_end + 3],
    ]);
    if crate::crc32c(&data[..value_end]) != stored_crc {
        return Err(StatefsError::Corrupted);
    }
    let key = core::str::from_utf8(&data[11..key_end]).map_err(|_| StatefsError::Corrupted)?.into();
    Ok(Some((JournalRecord { op, key, value: value.to_vec() }, total_len)))
}

// ============================================================================
// TxnTable — bounded in-flight transaction buffers (writer + replay shared)
// ============================================================================

struct TxnBuf {
    id: u64,
    /// Poisoned = cap violation seen; the id stays reserved so a later
    /// COMMIT is ignored instead of applying a partial buffer.
    poisoned: bool,
    bytes: usize,
    entries: Vec<(String, Vec<u8>)>,
}

/// Bounded table of open transactions. All limits are hard caps; violations
/// surface as errors (writer path) or deterministic poisoning (replay path).
pub(crate) struct TxnTable {
    txns: Vec<TxnBuf>,
    total_bytes: usize,
}

impl TxnTable {
    pub(crate) fn new() -> Self {
        Self { txns: Vec::new(), total_bytes: 0 }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.txns.is_empty()
    }

    pub(crate) fn open_count(&self) -> usize {
        self.txns.len()
    }

    pub(crate) fn clear(&mut self) {
        self.txns.clear();
        self.total_bytes = 0;
    }

    fn find(&mut self, id: u64) -> Option<&mut TxnBuf> {
        self.txns.iter_mut().find(|t| t.id == id)
    }

    fn begin(&mut self, id: u64) -> Result<(), StatefsError> {
        if self.txns.len() >= MAX_OPEN_TXNS {
            return Err(StatefsError::IoError);
        }
        self.txns.push(TxnBuf { id, poisoned: false, bytes: 0, entries: Vec::new() });
        Ok(())
    }

    /// Side-effect-free cap check for `append` (the writer path validates
    /// BEFORE journaling so journal bytes and buffer state never diverge).
    fn can_append(&self, id: u64, key: &str, chunk: &[u8]) -> Result<(), StatefsError> {
        if chunk.len() > MAX_TXN_CHUNK {
            return Err(StatefsError::ValueTooLarge);
        }
        let txn = self.txns.iter().find(|t| t.id == id).ok_or(StatefsError::NotFound)?;
        if txn.poisoned {
            return Err(StatefsError::IoError);
        }
        let added = match txn.entries.iter().find(|(k, _)| k == key) {
            Some((_, value)) => {
                if value.len().saturating_add(chunk.len()) > MAX_VALUE_SIZE {
                    return Err(StatefsError::ValueTooLarge);
                }
                chunk.len()
            }
            None => {
                if txn.entries.len() >= MAX_TXN_KEYS {
                    return Err(StatefsError::ValueTooLarge);
                }
                key.len().saturating_add(chunk.len())
            }
        };
        if txn.bytes.saturating_add(added) > MAX_TXN_BYTES
            || self.total_bytes.saturating_add(added) > MAX_TXN_TOTAL_BYTES
        {
            return Err(StatefsError::ValueTooLarge);
        }
        Ok(())
    }

    /// Append a chunk to `key`'s assembling value. Chunks for the same key
    /// concatenate in journal order; each new key counts toward the caps.
    fn append(&mut self, id: u64, key: &str, chunk: &[u8]) -> Result<(), StatefsError> {
        self.can_append(id, key, chunk)?;
        let Some(txn) = self.find(id) else {
            return Err(StatefsError::NotFound);
        };
        let added = match txn.entries.iter_mut().find(|(k, _)| k == key) {
            Some((_, value)) => {
                value.extend_from_slice(chunk);
                chunk.len()
            }
            None => {
                txn.entries.push((String::from(key), chunk.to_vec()));
                key.len().saturating_add(chunk.len())
            }
        };
        txn.bytes += added;
        self.total_bytes += added;
        Ok(())
    }

    /// Drop the buffer but keep the id reserved (replay cap violation).
    fn poison(&mut self, id: u64) {
        if let Some(txn) = self.find(id) {
            let freed = txn.bytes;
            txn.entries.clear();
            txn.bytes = 0;
            txn.poisoned = true;
            self.total_bytes = self.total_bytes.saturating_sub(freed);
        }
    }

    /// Remove the transaction; returns its entries if it was clean.
    fn take(&mut self, id: u64) -> Option<Vec<(String, Vec<u8>)>> {
        let idx = self.txns.iter().position(|t| t.id == id)?;
        let txn = self.txns.remove(idx);
        self.total_bytes = self.total_bytes.saturating_sub(txn.bytes);
        if txn.poisoned {
            None
        } else {
            Some(txn.entries)
        }
    }

    fn contains(&self, id: u64) -> bool {
        self.txns.iter().any(|t| t.id == id)
    }

    /// (buffered entry count, whether `key` already has bytes) for an open
    /// txn — the sealed-chunk index and single-chunk rule (TASK-0027).
    fn entry_state(&self, id: u64, key: &str) -> Option<(usize, bool)> {
        let txn = self.txns.iter().find(|t| t.id == id)?;
        Some((txn.entries.len(), txn.entries.iter().any(|(k, _)| k == key)))
    }
}

// ============================================================================
// Replayer — committed-only application of a record stream
// ============================================================================

/// Replay outcome (fed back into the engine after the scan).
pub(crate) struct ReplayOutcome {
    /// Next transaction id: max SEEN id + 1 (committed, aborted or torn —
    /// a discarded id must never be reused, nxfs discipline).
    pub(crate) next_txn: u64,
    /// Generation of the last Checkpoint record seen (0 = none).
    pub(crate) generation: u32,
    /// Count of ignored/discarded anomalies (orphan records, dropped txns).
    /// Feeds the future fsck orphan detection; not an error by itself.
    pub(crate) orphans: usize,
}

/// Applies a parsed record stream to a KV map with committed-only 2PC
/// semantics. v1 Put/Delete apply immediately (each is its own committed
/// record — DELETE stays non-transactional in v2a).
pub(crate) struct Replayer {
    txns: TxnTable,
    max_seen_txn: u64,
    generation: u32,
    orphans: usize,
}

impl Replayer {
    pub(crate) fn new() -> Self {
        Self { txns: TxnTable::new(), max_seen_txn: 0, generation: 0, orphans: 0 }
    }

    /// Apply one record. Never panics and never fails: semantic anomalies
    /// (unknown-txn COMMIT, cap violations) are deterministically ignored or
    /// poisoned per the documented contract.
    pub(crate) fn apply(
        &mut self,
        kv: &mut alloc::collections::BTreeMap<String, Vec<u8>>,
        record: JournalRecord,
    ) {
        match record.op {
            JournalOpCode::Put => {
                kv.insert(record.key, record.value);
            }
            JournalOpCode::Delete => {
                kv.remove(&record.key);
            }
            JournalOpCode::Checkpoint => {
                // Snapshot boundary: everything before is superseded.
                kv.clear();
                self.txns.clear();
                self.generation = read_u32(&record.value);
            }
            JournalOpCode::TxnPrepare => {
                let id = read_u64(&record.value);
                self.note_txn(id);
                if self.txns.contains(id) {
                    // Re-PREPARE of an open id orphans the open buffer.
                    self.orphans += 1;
                    let _ = self.txns.take(id);
                }
                if self.txns.begin(id).is_err() {
                    // Table full: the txn is untracked; its later records
                    // are ignored as orphans (bounded by design).
                    self.orphans += 1;
                }
            }
            JournalOpCode::TxnPayload => {
                let id = read_u64(&record.value);
                self.note_txn(id);
                let chunk = &record.value[8..];
                if self.txns.contains(id) {
                    if self.txns.append(id, &record.key, chunk).is_err() {
                        // Cap violation inside a group: poison the group.
                        self.txns.poison(id);
                        self.orphans += 1;
                    }
                } else {
                    self.orphans += 1;
                }
            }
            JournalOpCode::TxnCommit => {
                let id = read_u64(&record.value);
                self.note_txn(id);
                match self.txns.take(id) {
                    Some(entries) => {
                        for (key, value) in entries {
                            kv.insert(key, value);
                        }
                    }
                    // Unknown or poisoned txn: ignored, never half-applied.
                    None => self.orphans += 1,
                }
            }
            JournalOpCode::TxnAbort => {
                let id = read_u64(&record.value);
                self.note_txn(id);
                if !self.txns.contains(id) {
                    self.orphans += 1;
                }
                let _ = self.txns.take(id);
            }
            JournalOpCode::SyncBarrier => {}
        }
    }

    fn note_txn(&mut self, id: u64) {
        if id > self.max_seen_txn {
            self.max_seen_txn = id;
        }
    }

    /// TASK-0027: a sealed chunk that failed its AEAD verify poisons the
    /// whole transaction (both-or-neither — a tampered chunk must never
    /// half-apply). Counted like any other discarded-group anomaly.
    pub(crate) fn poison(&mut self, id: u64) {
        self.txns.poison(id);
        self.orphans += 1;
    }

    /// End of scan: any still-open transaction is discarded wholesale.
    pub(crate) fn finish(self) -> ReplayOutcome {
        let dropped = self.txns.open_count();
        ReplayOutcome {
            next_txn: self.max_seen_txn.saturating_add(1),
            generation: self.generation,
            orphans: self.orphans + dropped,
        }
    }
}

fn read_u64(value: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let len = core::cmp::min(value.len(), 8);
    buf[..len].copy_from_slice(&value[..len]);
    u64::from_le_bytes(buf)
}

fn read_u32(value: &[u8]) -> u32 {
    let mut buf = [0u8; 4];
    let len = core::cmp::min(value.len(), 4);
    buf[..len].copy_from_slice(&value[..len]);
    u32::from_le_bytes(buf)
}

// ============================================================================
// Writer-side transaction API (used by statefsd's txn wire ops next step)
// ============================================================================

impl<B: BlockDevice> JournalEngine<B> {
    /// Open a transaction: journals `TXN_PREPARE` and returns the txn id.
    /// Errors: `IoError` = open-txn cap reached or journal append failed.
    pub fn txn_begin(&mut self) -> Result<u64, StatefsError> {
        if self.txns.open_count() >= MAX_OPEN_TXNS {
            return Err(StatefsError::IoError);
        }
        let id = self.next_txn;
        self.append_bytes(&encode_prepare(id))?;
        self.next_txn = self.next_txn.saturating_add(1);
        self.txns.begin(id)?;
        Ok(id)
    }

    /// Append one value chunk for `key` to an open transaction. Chunks for
    /// the same key concatenate; nothing becomes visible before commit.
    /// Errors: `NotFound` = unknown txn id; `ValueTooLarge` = chunk/key/byte
    /// caps exceeded; key errors as in `put`.
    pub fn txn_append(&mut self, txn_id: u64, key: &str, chunk: &[u8]) -> Result<(), StatefsError> {
        Self::validate_key(key)?;
        // TASK-0027: enrolled keys are sealed per chunk with nonce input
        // (txn_id, entry index). Sealing forbids concatenation, so enrolled
        // values must arrive as ONE complete chunk (envelope discipline).
        if let Some(class) = self.enc.as_ref().and_then(|c| c.class_for(key)) {
            let (idx, present) =
                self.txns.entry_state(txn_id, key).ok_or(StatefsError::NotFound)?;
            if present || chunk.len().saturating_add(crate::enc::SEALED_OVERHEAD) > MAX_VALUE_SIZE {
                return Err(StatefsError::ValueTooLarge);
            }
            let mut sealed = core::mem::take(&mut self.seal_scratch);
            let result = match self.enc.as_ref() {
                Some(ctx) => {
                    crate::enc::seal_into(ctx, class, key, txn_id, idx as u32, chunk, &mut sealed)
                }
                None => Err(StatefsError::IntegrityViolation),
            };
            let outcome = result.and_then(|()| {
                self.txns.can_append(txn_id, key, &sealed)?;
                let record = encode_payload(txn_id, key, &sealed)?;
                self.append_bytes(&record)?;
                self.txns.append(txn_id, key, &sealed)
            });
            self.seal_scratch = sealed;
            return outcome;
        }
        // Validate against the buffer caps FIRST (side-effect free), then
        // journal the payload, then mirror into the buffer — journal bytes
        // and buffer state must never diverge.
        self.txns.can_append(txn_id, key, chunk)?;
        let record = encode_payload(txn_id, key, chunk)?;
        self.append_bytes(&record)?;
        self.txns.append(txn_id, key, chunk)
    }

    /// Commit: journals `TXN_COMMIT`, then applies all buffered entries to
    /// the in-memory state atomically. Errors: `NotFound` = unknown txn id;
    /// `IoError` = journal append failed (txn stays open for abort/retry).
    pub fn txn_commit(&mut self, txn_id: u64) -> Result<(), StatefsError> {
        if !self.txns.contains(txn_id) {
            return Err(StatefsError::NotFound);
        }
        self.append_bytes(&encode_commit(txn_id))?;
        if let Some(entries) = self.txns.take(txn_id) {
            for (key, value) in entries {
                self.kv.insert(key, value);
            }
        }
        Ok(())
    }

    /// Abort: journals `TXN_ABORT` and discards the buffered entries.
    /// Errors: `NotFound` = unknown txn id.
    pub fn txn_abort(&mut self, txn_id: u64) -> Result<(), StatefsError> {
        if !self.txns.contains(txn_id) {
            return Err(StatefsError::NotFound);
        }
        self.append_bytes(&encode_abort(txn_id))?;
        let _ = self.txns.take(txn_id);
        Ok(())
    }

    /// Journal a `SYNC{}` barrier record and flush the device.
    pub fn sync_barrier(&mut self) -> Result<(), StatefsError> {
        self.append_bytes(&encode_sync_barrier())?;
        self.sync()
    }

    /// Number of currently open (prepared, uncommitted) transactions.
    pub fn open_txns(&self) -> usize {
        self.txns.open_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_record_shapes_roundtrip() {
        for bytes in [
            encode_prepare(7),
            encode_payload(7, "/state/test/a", b"chunk").expect("payload"),
            encode_commit(7),
            encode_abort(7),
            encode_sync_barrier(),
            encode_checkpoint(3, 42),
        ] {
            let (record, consumed) = parse_record(&bytes).expect("parse").expect("record");
            assert_eq!(consumed, bytes.len());
            // Ids survive the trip.
            let id_bearing = matches!(
                record.op,
                JournalOpCode::TxnPrepare | JournalOpCode::TxnCommit | JournalOpCode::TxnAbort
            );
            assert!(!id_bearing || read_u64(&record.value) == 7);
        }
    }

    #[test]
    fn test_reject_bad_shape_is_corrupted() {
        // PREPARE with a 4-byte value: valid CRC, invalid shape.
        let bytes = encode_record(JournalOpCode::TxnPrepare, "", &[1, 2, 3, 4]);
        assert!(matches!(parse_record(&bytes), Err(StatefsError::Corrupted)));
    }

    #[test]
    fn txn_table_caps_are_hard() {
        let mut table = TxnTable::new();
        for id in 0..MAX_OPEN_TXNS as u64 {
            table.begin(id).expect("begin");
        }
        assert_eq!(table.begin(99), Err(StatefsError::IoError));
        // Per-txn key cap.
        for i in 0..MAX_TXN_KEYS {
            let key = alloc::format!("/state/t/{i}");
            table.append(0, &key, b"x").expect("append");
        }
        assert_eq!(table.append(0, "/state/t/overflow", b"x"), Err(StatefsError::ValueTooLarge));
        // Same-key chunks concatenate without a new key slot.
        table.append(0, "/state/t/0", b"y").expect("append same key");
    }
}
