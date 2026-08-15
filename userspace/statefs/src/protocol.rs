// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS IPC protocol framing (statefsd wire v1 + nonce v2)
//! OWNERS: @runtime
//! STATUS: Functional (host-first)
//! API_STABILITY: Stable (v1.0) — wire bytes are a service contract
//! TEST_COVERAGE: Exercised by statefsd contract tests + envelope tests
//!
//! Moved verbatim out of `lib.rs` (TASK-0026 structure split); the module
//! path `statefs::protocol` and every byte on the wire are unchanged.

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::str;

use super::{StatefsError, MAX_KEY_LEN, MAX_VALUE_SIZE};

/// Transaction wire ops (TASK-0026 step 4; appended opcodes 7–10).
pub mod txn;

pub const MAGIC0: u8 = b'S';
pub const MAGIC1: u8 = b'F';
pub const VERSION: u8 = 1;
pub const VERSION_V2: u8 = 2;

pub const OP_PUT: u8 = 1;
pub const OP_GET: u8 = 2;
pub const OP_DEL: u8 = 3;
pub const OP_LIST: u8 = 4;
pub const OP_SYNC: u8 = 5;
pub const OP_REOPEN: u8 = 6;

pub const STATUS_OK: u8 = 0;
pub const STATUS_NOT_FOUND: u8 = 1;
pub const STATUS_ACCESS_DENIED: u8 = 2;
pub const STATUS_VALUE_TOO_LARGE: u8 = 3;
pub const STATUS_KEY_TOO_LONG: u8 = 4;
pub const STATUS_INVALID_KEY: u8 = 5;
pub const STATUS_MALFORMED: u8 = 6;
pub const STATUS_IO_ERROR: u8 = 7;
pub const STATUS_UNSUPPORTED: u8 = 8;
pub const STATUS_INTEGRITY_VIOLATION: u8 = 9;
pub const STATUS_ROLLBACK_DETECTED: u8 = 10;

pub const MAX_LIST_LIMIT: u16 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request<'a> {
    Put { key: &'a str, value: &'a [u8] },
    Get { key: &'a str },
    Delete { key: &'a str },
    List { prefix: &'a str, limit: u16 },
    Sync,
    Reopen,
}

fn decode_request_no_nonce(frame: &[u8]) -> Result<Request<'_>, u8> {
    if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
        return Err(STATUS_MALFORMED);
    }
    let op = frame[3];
    let payload = &frame[4..];
    match op {
        OP_PUT => decode_put_payload(payload),
        OP_GET => decode_key_only_payload(payload).map(|key| Request::Get { key }),
        OP_DEL => decode_key_only_payload(payload).map(|key| Request::Delete { key }),
        OP_LIST => decode_list_payload(payload),
        OP_SYNC => {
            if !payload.is_empty() {
                Err(STATUS_MALFORMED)
            } else {
                Ok(Request::Sync)
            }
        }
        OP_REOPEN => {
            if !payload.is_empty() {
                Err(STATUS_MALFORMED)
            } else {
                Ok(Request::Reopen)
            }
        }
        _ => Err(STATUS_UNSUPPORTED),
    }
}

/// Decode a request and (optionally) a trailing u64 nonce (little-endian).
///
/// Backward compatible:
/// - If the frame matches the v1 shape exactly, nonce is `None`.
pub fn decode_request_with_nonce(frame: &[u8]) -> Result<(Request<'_>, Option<u64>), u8> {
    if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Err(STATUS_MALFORMED);
    }
    match frame[2] {
        VERSION => decode_request_no_nonce(frame).map(|r| (r, None)),
        VERSION_V2 => {
            if frame.len() < 12 {
                return Err(STATUS_MALFORMED);
            }
            let op = frame[3];
            let mut nb = [0u8; 8];
            nb.copy_from_slice(&frame[4..12]);
            let nonce = u64::from_le_bytes(nb);
            let payload = &frame[12..];
            let req = match op {
                OP_PUT => decode_put_payload(payload),
                OP_GET => decode_key_only_payload(payload).map(|key| Request::Get { key }),
                OP_DEL => decode_key_only_payload(payload).map(|key| Request::Delete { key }),
                OP_LIST => decode_list_payload(payload),
                OP_SYNC => {
                    if !payload.is_empty() {
                        Err(STATUS_MALFORMED)
                    } else {
                        Ok(Request::Sync)
                    }
                }
                OP_REOPEN => {
                    if !payload.is_empty() {
                        Err(STATUS_MALFORMED)
                    } else {
                        Ok(Request::Reopen)
                    }
                }
                _ => Err(STATUS_UNSUPPORTED),
            }?;
            Ok((req, Some(nonce)))
        }
        _ => Err(STATUS_MALFORMED),
    }
}

pub fn decode_request(frame: &[u8]) -> Result<Request<'_>, u8> {
    decode_request_with_nonce(frame).map(|(r, _)| r)
}

pub fn encode_status_response(op: u8, status: u8) -> Vec<u8> {
    vec![MAGIC0, MAGIC1, VERSION, op | 0x80, status]
}

pub fn encode_status_response_with_nonce(op: u8, status: u8, nonce: Option<u64>) -> Vec<u8> {
    if let Some(n) = nonce {
        let mut out = Vec::with_capacity(13);
        out.push(MAGIC0);
        out.push(MAGIC1);
        out.push(VERSION_V2);
        out.push(op | 0x80);
        out.push(status);
        out.extend_from_slice(&n.to_le_bytes());
        out
    } else {
        encode_status_response(op, status)
    }
}

pub fn encode_get_response(status: u8, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(9 + value.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_GET | 0x80);
    out.push(status);
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value);
    out
}

pub fn encode_get_response_with_nonce(status: u8, value: &[u8], nonce: Option<u64>) -> Vec<u8> {
    if let Some(n) = nonce {
        let mut out = Vec::with_capacity(17 + value.len());
        out.push(MAGIC0);
        out.push(MAGIC1);
        out.push(VERSION_V2);
        out.push(OP_GET | 0x80);
        out.push(status);
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value);
        out
    } else {
        encode_get_response(status, value)
    }
}

pub fn encode_list_response(status: u8, keys: &[String], max_bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_LIST | 0x80);
    out.push(status);

    // Placeholder for count
    out.extend_from_slice(&0u16.to_le_bytes());
    let count_pos = 5;
    let mut count: u16 = 0;

    for key in keys {
        let key_bytes = key.as_bytes();
        if key_bytes.len() > MAX_KEY_LEN {
            continue;
        }
        let entry_len = 2usize.saturating_add(key_bytes.len());
        if out.len().saturating_add(entry_len) > max_bytes {
            break;
        }
        out.extend_from_slice(&(key_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(key_bytes);
        count = count.saturating_add(1);
        if count == u16::MAX {
            break;
        }
    }

    let count_bytes = count.to_le_bytes();
    if out.len() >= count_pos + 2 {
        out[count_pos] = count_bytes[0];
        out[count_pos + 1] = count_bytes[1];
    }
    out
}

pub fn encode_list_response_with_nonce(
    status: u8,
    keys: &[String],
    max_bytes: usize,
    nonce: Option<u64>,
) -> Vec<u8> {
    if let Some(n) = nonce {
        // v2 layout:
        // [MAGIC0, MAGIC1, VERSION_V2, OP_LIST|0x80, status, nonce:u64, count:u16, entries...]
        let mut out = Vec::with_capacity(15);
        out.push(MAGIC0);
        out.push(MAGIC1);
        out.push(VERSION_V2);
        out.push(OP_LIST | 0x80);
        out.push(status);
        out.extend_from_slice(&n.to_le_bytes());

        // Placeholder for count.
        out.extend_from_slice(&0u16.to_le_bytes());
        let count_pos = 13;
        let mut count: u16 = 0;

        for key in keys {
            let key_bytes = key.as_bytes();
            if key_bytes.len() > MAX_KEY_LEN {
                continue;
            }
            let entry_len = 2usize.saturating_add(key_bytes.len());
            if out.len().saturating_add(entry_len) > max_bytes {
                break;
            }
            out.extend_from_slice(&(key_bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(key_bytes);
            count = count.saturating_add(1);
            if count == u16::MAX {
                break;
            }
        }

        let count_bytes = count.to_le_bytes();
        if out.len() >= count_pos + 2 {
            out[count_pos] = count_bytes[0];
            out[count_pos + 1] = count_bytes[1];
        }
        out
    } else {
        encode_list_response(status, keys, max_bytes)
    }
}

pub fn decode_status_response(expected_op: u8, frame: &[u8]) -> Result<u8, StatefsError> {
    if frame.len() < 5 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Err(StatefsError::Corrupted);
    }
    if frame[3] != (expected_op | 0x80) {
        return Err(StatefsError::Corrupted);
    }
    match frame[2] {
        VERSION => Ok(frame[4]),
        VERSION_V2 => {
            if frame.len() < 13 {
                return Err(StatefsError::Corrupted);
            }
            Ok(frame[4])
        }
        _ => Err(StatefsError::Corrupted),
    }
}

pub fn decode_get_response(frame: &[u8]) -> Result<Vec<u8>, StatefsError> {
    if frame.len() < 9 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Err(StatefsError::Corrupted);
    }
    if frame[3] != (OP_GET | 0x80) {
        return Err(StatefsError::Corrupted);
    }
    match frame[2] {
        VERSION => {
            if frame.len() < 9 {
                return Err(StatefsError::Corrupted);
            }
            let status = frame[4];
            if status != STATUS_OK {
                return Err(error_from_status(status));
            }
            let val_len = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]) as usize;
            if val_len > MAX_VALUE_SIZE || frame.len() != 9 + val_len {
                return Err(StatefsError::Corrupted);
            }
            Ok(frame[9..9 + val_len].to_vec())
        }
        VERSION_V2 => {
            if frame.len() < 17 {
                return Err(StatefsError::Corrupted);
            }
            let status = frame[4];
            if status != STATUS_OK {
                return Err(error_from_status(status));
            }
            let val_len = u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]) as usize;
            if val_len > MAX_VALUE_SIZE || frame.len() != 17 + val_len {
                return Err(StatefsError::Corrupted);
            }
            Ok(frame[17..17 + val_len].to_vec())
        }
        _ => Err(StatefsError::Corrupted),
    }
}

pub fn decode_list_response(frame: &[u8]) -> Result<Vec<String>, StatefsError> {
    if frame.len() < 7 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Err(StatefsError::Corrupted);
    }
    if frame[3] != (OP_LIST | 0x80) {
        return Err(StatefsError::Corrupted);
    }
    let (count, mut pos) = match frame[2] {
        VERSION => {
            if frame.len() < 7 {
                return Err(StatefsError::Corrupted);
            }
            (u16::from_le_bytes([frame[5], frame[6]]) as usize, 7usize)
        }
        VERSION_V2 => {
            if frame.len() < 15 {
                return Err(StatefsError::Corrupted);
            }
            (u16::from_le_bytes([frame[13], frame[14]]) as usize, 15usize)
        }
        _ => return Err(StatefsError::Corrupted),
    };
    let status = frame[4];
    if status != STATUS_OK {
        return Err(error_from_status(status));
    }
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        if pos + 2 > frame.len() {
            return Err(StatefsError::Corrupted);
        }
        let key_len = u16::from_le_bytes([frame[pos], frame[pos + 1]]) as usize;
        pos += 2;
        if key_len > MAX_KEY_LEN || pos + key_len > frame.len() {
            return Err(StatefsError::Corrupted);
        }
        let key = str::from_utf8(&frame[pos..pos + key_len])
            .map_err(|_| StatefsError::Corrupted)?
            .to_string();
        pos += key_len;
        keys.push(key);
    }
    Ok(keys)
}

pub fn encode_put_request(key: &str, value: &[u8]) -> Result<Vec<u8>, StatefsError> {
    if key.len() > MAX_KEY_LEN {
        return Err(StatefsError::KeyTooLong);
    }
    if value.len() > MAX_VALUE_SIZE {
        return Err(StatefsError::ValueTooLarge);
    }
    let mut out = Vec::with_capacity(10 + key.len() + value.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_PUT);
    out.extend_from_slice(&(key.len() as u16).to_le_bytes());
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    out.extend_from_slice(value);
    Ok(out)
}

pub fn encode_key_only_request(op: u8, key: &str) -> Result<Vec<u8>, StatefsError> {
    if key.len() > MAX_KEY_LEN {
        return Err(StatefsError::KeyTooLong);
    }
    let mut out = Vec::with_capacity(6 + key.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op);
    out.extend_from_slice(&(key.len() as u16).to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    Ok(out)
}

pub fn encode_list_request(prefix: &str, limit: u16) -> Result<Vec<u8>, StatefsError> {
    if prefix.len() > MAX_KEY_LEN {
        return Err(StatefsError::KeyTooLong);
    }
    let limit = if limit == 0 { 1 } else { limit.min(MAX_LIST_LIMIT) };
    let mut out = Vec::with_capacity(8 + prefix.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_LIST);
    out.extend_from_slice(&(prefix.len() as u16).to_le_bytes());
    out.extend_from_slice(&limit.to_le_bytes());
    out.extend_from_slice(prefix.as_bytes());
    Ok(out)
}

pub fn encode_sync_request() -> Vec<u8> {
    vec![MAGIC0, MAGIC1, VERSION, OP_SYNC]
}

pub fn encode_reopen_request() -> Vec<u8> {
    vec![MAGIC0, MAGIC1, VERSION, OP_REOPEN]
}

pub fn status_from_error(err: StatefsError) -> u8 {
    match err {
        StatefsError::NotFound => STATUS_NOT_FOUND,
        StatefsError::AccessDenied => STATUS_ACCESS_DENIED,
        StatefsError::ValueTooLarge => STATUS_VALUE_TOO_LARGE,
        StatefsError::KeyTooLong => STATUS_KEY_TOO_LONG,
        StatefsError::InvalidKey => STATUS_INVALID_KEY,
        StatefsError::IoError => STATUS_IO_ERROR,
        StatefsError::Corrupted => STATUS_MALFORMED,
        StatefsError::ReplayLimitExceeded => STATUS_IO_ERROR,
        StatefsError::IntegrityViolation => STATUS_INTEGRITY_VIOLATION,
        StatefsError::RollbackDetected => STATUS_ROLLBACK_DETECTED,
    }
}

pub fn error_from_status(status: u8) -> StatefsError {
    match status {
        STATUS_NOT_FOUND => StatefsError::NotFound,
        STATUS_ACCESS_DENIED => StatefsError::AccessDenied,
        STATUS_VALUE_TOO_LARGE => StatefsError::ValueTooLarge,
        STATUS_KEY_TOO_LONG => StatefsError::KeyTooLong,
        STATUS_INVALID_KEY => StatefsError::InvalidKey,
        STATUS_IO_ERROR => StatefsError::IoError,
        STATUS_MALFORMED | STATUS_UNSUPPORTED => StatefsError::Corrupted,
        STATUS_INTEGRITY_VIOLATION => StatefsError::IntegrityViolation,
        STATUS_ROLLBACK_DETECTED => StatefsError::RollbackDetected,
        _ => StatefsError::Corrupted,
    }
}

fn decode_put_payload(payload: &[u8]) -> Result<Request<'_>, u8> {
    // payload: key_len:u16, val_len:u32, key, value
    if payload.len() < 6 {
        return Err(STATUS_MALFORMED);
    }
    let key_len = u16::from_le_bytes([payload[0], payload[1]]) as usize;
    let val_len = u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]) as usize;
    if key_len == 0 {
        return Err(STATUS_MALFORMED);
    }
    if key_len > MAX_KEY_LEN {
        return Err(STATUS_KEY_TOO_LONG);
    }
    if val_len > MAX_VALUE_SIZE {
        return Err(STATUS_VALUE_TOO_LARGE);
    }
    let expected = 6usize.saturating_add(key_len).saturating_add(val_len);
    if payload.len() != expected {
        return Err(STATUS_MALFORMED);
    }
    let key_start = 6;
    let key_end = key_start + key_len;
    let key = str::from_utf8(&payload[key_start..key_end]).map_err(|_| STATUS_MALFORMED)?;
    let value = &payload[key_end..expected];
    Ok(Request::Put { key, value })
}

fn decode_key_only_payload(payload: &[u8]) -> Result<&str, u8> {
    // payload: key_len:u16, key
    if payload.len() < 2 {
        return Err(STATUS_MALFORMED);
    }
    let key_len = u16::from_le_bytes([payload[0], payload[1]]) as usize;
    if key_len == 0 {
        return Err(STATUS_MALFORMED);
    }
    if key_len > MAX_KEY_LEN {
        return Err(STATUS_KEY_TOO_LONG);
    }
    let expected = 2usize.saturating_add(key_len);
    if payload.len() != expected {
        return Err(STATUS_MALFORMED);
    }
    let key_start = 2;
    let key_end = key_start + key_len;
    str::from_utf8(&payload[key_start..key_end]).map_err(|_| STATUS_MALFORMED)
}

fn decode_list_payload(payload: &[u8]) -> Result<Request<'_>, u8> {
    // payload: prefix_len:u16, limit:u16, prefix
    if payload.len() < 4 {
        return Err(STATUS_MALFORMED);
    }
    let prefix_len = u16::from_le_bytes([payload[0], payload[1]]) as usize;
    let limit = u16::from_le_bytes([payload[2], payload[3]]);
    if prefix_len > MAX_KEY_LEN {
        return Err(STATUS_KEY_TOO_LONG);
    }
    let expected = 4usize.saturating_add(prefix_len);
    if payload.len() != expected {
        return Err(STATUS_MALFORMED);
    }
    let prefix = str::from_utf8(&payload[4..expected]).map_err(|_| STATUS_MALFORMED)?;
    let limit = if limit == 0 { 1 } else { limit.min(MAX_LIST_LIMIT) };
    Ok(Request::List { prefix, limit })
}
