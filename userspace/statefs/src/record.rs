// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS record serialization (v1 framing, shared by journal
//!   appends, txn records, and compaction snapshots). Split out of
//!   journal_v2.rs so the append-in-place encoder exists exactly once:
//!   os-lite services run on a bump allocator that never frees, so hot
//!   paths must encode into persistent, capacity-reused buffers instead
//!   of allocating a fresh `Vec` per record.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (crate-internal framing helper)
//! TEST_COVERAGE: exercised by every journal/replay/compaction test;
//!   round-trip via journal_v2::parse_record
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use alloc::vec::Vec;

use crate::journal_v2::JournalOpCode;
use crate::RECORD_HEADER_SIZE;

/// Serialize one record on the v1 framing:
/// `NXSF | op | keylen u16 | vallen u32 | key | value | crc32c`.
pub fn encode_record(op: JournalOpCode, key: &str, value: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(RECORD_HEADER_SIZE + key.len() + value.len());
    encode_record_into(&mut buf, op, key, value);
    buf
}

/// Encode `TXN_PREPARE{txn_id}`.
pub fn encode_prepare(txn_id: u64) -> Vec<u8> {
    encode_record(JournalOpCode::TxnPrepare, "", &txn_id.to_le_bytes())
}

/// Encode `TXN_PAYLOAD{txn_id, key, chunk}`. Chunk must be `<= MAX_TXN_CHUNK`.
pub fn encode_payload(
    txn_id: u64,
    key: &str,
    chunk: &[u8],
) -> Result<Vec<u8>, crate::StatefsError> {
    if chunk.len() > crate::journal_v2::MAX_TXN_CHUNK {
        return Err(crate::StatefsError::ValueTooLarge);
    }
    let mut value = Vec::with_capacity(8 + chunk.len());
    value.extend_from_slice(&txn_id.to_le_bytes());
    value.extend_from_slice(chunk);
    Ok(encode_record(JournalOpCode::TxnPayload, key, &value))
}

/// Encode `TXN_COMMIT{txn_id}`.
pub fn encode_commit(txn_id: u64) -> Vec<u8> {
    encode_record(JournalOpCode::TxnCommit, "", &txn_id.to_le_bytes())
}

/// Encode `TXN_ABORT{txn_id}`.
pub fn encode_abort(txn_id: u64) -> Vec<u8> {
    encode_record(JournalOpCode::TxnAbort, "", &txn_id.to_le_bytes())
}

/// Encode `SYNC{}` (journal durability-barrier record).
pub fn encode_sync_barrier() -> Vec<u8> {
    encode_record(JournalOpCode::SyncBarrier, "", &[])
}

/// Encode `CHECKPOINT{gen, entries}` (snapshot boundary, compaction only).
pub fn encode_checkpoint(generation: u32, entries: u32) -> Vec<u8> {
    let mut value = Vec::with_capacity(8);
    value.extend_from_slice(&generation.to_le_bytes());
    value.extend_from_slice(&entries.to_le_bytes());
    encode_record(JournalOpCode::Checkpoint, "", &value)
}

/// Append one framed record to `buf` (no fresh allocation beyond `buf`'s
/// own growth — pass a persistent scratch on hot paths). The CRC covers
/// only the appended record, so records can be packed back to back.
pub fn encode_record_into(buf: &mut Vec<u8>, op: JournalOpCode, key: &str, value: &[u8]) {
    let key_bytes = key.as_bytes();
    let start = buf.len();
    buf.extend_from_slice(&crate::JOURNAL_MAGIC.to_le_bytes());
    buf.push(op as u8);
    buf.extend_from_slice(&(key_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
    buf.extend_from_slice(key_bytes);
    buf.extend_from_slice(value);
    let crc = crate::crc32c(&buf[start..]);
    buf.extend_from_slice(&crc.to_le_bytes());
}
