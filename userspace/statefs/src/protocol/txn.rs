// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS transaction wire ops (statefsd wire, TASK-0026 step 4)
//! OWNERS: @runtime
//! STATUS: Functional (host-first)
//! API_STABILITY: Stable once shipped — wire bytes are a service contract
//! TEST_COVERAGE: statefsd tests/txn_contract.rs + QEMU marker ladder
//!
//! Opcodes APPENDED to the frozen `SF` op table (never renumbered):
//! `TXN_BEGIN = 7`, `TXN_PUT = 8`, `TXN_COMMIT = 9`, `TXN_ABORT = 10`.
//! Both wire framings carry them (v1 and the nonce-correlated v2 of
//! RFC-0019). Payload shapes and the appended `STATUS_TXN_LIMIT` are
//! documented normatively in docs/storage/statefs.md §Journal v2
//! → "Service wire ops".

use alloc::vec;
use alloc::vec::Vec;

use super::{
    MAGIC0, MAGIC1, STATUS_KEY_TOO_LONG, STATUS_MALFORMED, STATUS_UNSUPPORTED,
    STATUS_VALUE_TOO_LARGE, VERSION, VERSION_V2,
};
use crate::journal_v2::MAX_TXN_CHUNK;
use crate::{StatefsError, MAX_KEY_LEN};

/// Opens a transaction; the response carries the fresh txn id.
pub const OP_TXN_BEGIN: u8 = 7;
/// One value chunk for a key inside an open transaction.
pub const OP_TXN_PUT: u8 = 8;
/// Commits a transaction (all buffered keys become visible atomically).
pub const OP_TXN_COMMIT: u8 = 9;
/// Aborts a transaction (all buffered keys are discarded).
pub const OP_TXN_ABORT: u8 = 10;

/// Appended status: the open-transaction cap is reached at `TXN_BEGIN`.
/// Distinct from `STATUS_IO_ERROR` on purpose — "retry after another txn
/// commits/aborts" is not a device failure.
pub const STATUS_TXN_LIMIT: u8 = 11;

/// Returns true for the transaction request opcodes.
#[must_use]
pub fn is_txn_op(op: u8) -> bool {
    matches!(op, OP_TXN_BEGIN..=OP_TXN_ABORT)
}

/// A decoded transaction request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxnRequest<'a> {
    /// `TXN_BEGIN` — empty payload.
    Begin,
    /// `TXN_PUT{txn_id, key, chunk}` — one value chunk (chunks for the same
    /// key concatenate engine-side, nothing visible before commit).
    Put { txn_id: u64, key: &'a str, chunk: &'a [u8] },
    /// `TXN_COMMIT{txn_id}`.
    Commit { txn_id: u64 },
    /// `TXN_ABORT{txn_id}`.
    Abort { txn_id: u64 },
}

/// Decode a transaction request (v1 or nonce-correlated v2 framing).
/// Errors carry the wire status to answer with.
pub fn decode_txn_request(frame: &[u8]) -> Result<(TxnRequest<'_>, Option<u64>), u8> {
    if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Err(STATUS_MALFORMED);
    }
    let (op, nonce, payload) = match frame[2] {
        VERSION => (frame[3], None, &frame[4..]),
        VERSION_V2 => {
            if frame.len() < 12 {
                return Err(STATUS_MALFORMED);
            }
            let mut nb = [0u8; 8];
            nb.copy_from_slice(&frame[4..12]);
            (frame[3], Some(u64::from_le_bytes(nb)), &frame[12..])
        }
        _ => return Err(STATUS_MALFORMED),
    };
    let request = match op {
        OP_TXN_BEGIN => {
            if payload.is_empty() {
                Ok(TxnRequest::Begin)
            } else {
                Err(STATUS_MALFORMED)
            }
        }
        OP_TXN_PUT => decode_txn_put_payload(payload),
        OP_TXN_COMMIT => decode_id_payload(payload).map(|txn_id| TxnRequest::Commit { txn_id }),
        OP_TXN_ABORT => decode_id_payload(payload).map(|txn_id| TxnRequest::Abort { txn_id }),
        _ => Err(STATUS_UNSUPPORTED),
    }?;
    Ok((request, nonce))
}

/// Encode a `TXN_BEGIN` request (v1 framing; clients may nonce-upgrade).
#[must_use]
pub fn encode_txn_begin_request() -> Vec<u8> {
    vec![MAGIC0, MAGIC1, VERSION, OP_TXN_BEGIN]
}

/// Encode a `TXN_PUT` request. Bounded client-side: key length and chunk
/// size are validated before any bytes are produced.
pub fn encode_txn_put_request(
    txn_id: u64,
    key: &str,
    chunk: &[u8],
) -> Result<Vec<u8>, StatefsError> {
    if key.is_empty() || key.len() > MAX_KEY_LEN {
        return Err(StatefsError::KeyTooLong);
    }
    if chunk.len() > MAX_TXN_CHUNK {
        return Err(StatefsError::ValueTooLarge);
    }
    let mut out = Vec::with_capacity(18 + key.len() + chunk.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_TXN_PUT);
    out.extend_from_slice(&txn_id.to_le_bytes());
    out.extend_from_slice(&(key.len() as u16).to_le_bytes());
    out.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    out.extend_from_slice(chunk);
    Ok(out)
}

/// Encode a `TXN_COMMIT` request.
#[must_use]
pub fn encode_txn_commit_request(txn_id: u64) -> Vec<u8> {
    encode_id_request(OP_TXN_COMMIT, txn_id)
}

/// Encode a `TXN_ABORT` request.
#[must_use]
pub fn encode_txn_abort_request(txn_id: u64) -> Vec<u8> {
    encode_id_request(OP_TXN_ABORT, txn_id)
}

/// Encode the `TXN_BEGIN` response: status + txn id (0 unless status is OK).
#[must_use]
pub fn encode_txn_begin_response(status: u8, txn_id: u64, nonce: Option<u64>) -> Vec<u8> {
    let mut out = super::encode_status_response_with_nonce(OP_TXN_BEGIN, status, nonce);
    out.extend_from_slice(&txn_id.to_le_bytes());
    out
}

/// Decode the `TXN_BEGIN` response into `(status, txn_id)`.
pub fn decode_txn_begin_response(frame: &[u8]) -> Result<(u8, u64), StatefsError> {
    if frame.len() < 5 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Err(StatefsError::Corrupted);
    }
    if frame[3] != (OP_TXN_BEGIN | 0x80) {
        return Err(StatefsError::Corrupted);
    }
    let id_off = match frame[2] {
        VERSION => 5usize,
        VERSION_V2 => 13usize,
        _ => return Err(StatefsError::Corrupted),
    };
    if frame.len() != id_off + 8 {
        return Err(StatefsError::Corrupted);
    }
    let mut ib = [0u8; 8];
    ib.copy_from_slice(&frame[id_off..id_off + 8]);
    Ok((frame[4], u64::from_le_bytes(ib)))
}

fn encode_id_request(op: u8, txn_id: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op);
    out.extend_from_slice(&txn_id.to_le_bytes());
    out
}

fn decode_id_payload(payload: &[u8]) -> Result<u64, u8> {
    if payload.len() != 8 {
        return Err(STATUS_MALFORMED);
    }
    let mut ib = [0u8; 8];
    ib.copy_from_slice(payload);
    Ok(u64::from_le_bytes(ib))
}

fn decode_txn_put_payload(payload: &[u8]) -> Result<TxnRequest<'_>, u8> {
    // payload: txn_id:u64, key_len:u16, chunk_len:u32, key, chunk
    if payload.len() < 14 {
        return Err(STATUS_MALFORMED);
    }
    let mut ib = [0u8; 8];
    ib.copy_from_slice(&payload[0..8]);
    let txn_id = u64::from_le_bytes(ib);
    let key_len = u16::from_le_bytes([payload[8], payload[9]]) as usize;
    let chunk_len =
        u32::from_le_bytes([payload[10], payload[11], payload[12], payload[13]]) as usize;
    if key_len == 0 {
        return Err(STATUS_MALFORMED);
    }
    if key_len > MAX_KEY_LEN {
        return Err(STATUS_KEY_TOO_LONG);
    }
    if chunk_len > MAX_TXN_CHUNK {
        return Err(STATUS_VALUE_TOO_LARGE);
    }
    let expected = 14usize.saturating_add(key_len).saturating_add(chunk_len);
    if payload.len() != expected {
        return Err(STATUS_MALFORMED);
    }
    let key_end = 14 + key_len;
    let key = core::str::from_utf8(&payload[14..key_end]).map_err(|_| STATUS_MALFORMED)?;
    let chunk = &payload[key_end..expected];
    Ok(TxnRequest::Put { txn_id, key, chunk })
}
