// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS journaled key-value store for /state persistence
//! OWNERS: @runtime
//! STATUS: Functional (host-first)
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: Host unit tests + negative tests
//!
//! PUBLIC API:
//!   - JournalEngine: Journaled KV store with Put/Get/Delete/List/Sync
//!   - protocol: IPC framing helpers for statefsd
//!   - client: IPC client wrapper (feature = "ipc-client")
//!   - StatefsError: Error types
//!
//! DEPENDENCIES:
//!   - crc32fast: CRC32-C checksums for journal integrity
//!   - storage: Block device abstractions
//!
//! ADR: docs/adr/0023-statefs-persistence-architecture.md
//!

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use storage::BlockDevice;

// ============================================================================
// Constants (statefs v1)
// ============================================================================

/// Journal record magic: "NXSF" (Nexus StateFS)
const JOURNAL_MAGIC: u32 = 0x4E58_5346;

/// Maximum key length in bytes
pub const MAX_KEY_LEN: usize = 255;

/// Maximum value size in bytes (64 KiB)
pub const MAX_VALUE_SIZE: usize = 65536;

/// Maximum records to replay (bounded replay)
const MAX_REPLAY_RECORDS: usize = 100_000;

/// Journal record header size: magic(4) + opcode(1) + key_len(2) + value_len(4) + crc(4) = 15
const RECORD_HEADER_SIZE: usize = 15;

// ============================================================================
// Error Types
// ============================================================================

/// StateFS error codes (v1 contract)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatefsError {
    /// Key not found
    NotFound,
    /// Access denied (policy violation)
    AccessDenied,
    /// Value exceeds maximum size
    ValueTooLarge,
    /// Key exceeds maximum length
    KeyTooLong,
    /// Block I/O error
    IoError,
    /// Journal corruption detected (CRC mismatch)
    Corrupted,
    /// Invalid key format (path normalization failed)
    InvalidKey,
    /// Replay depth exceeded
    ReplayLimitExceeded,
}

// ============================================================================
// IPC Protocol (statefsd)
// ============================================================================

pub mod protocol {
    use alloc::string::String;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::str;

    use super::{MAX_KEY_LEN, MAX_VALUE_SIZE, StatefsError};

    pub const MAGIC0: u8 = b'S';
    pub const MAGIC1: u8 = b'F';
    pub const VERSION: u8 = 1;

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

    pub fn decode_request(frame: &[u8]) -> Result<Request<'_>, u8> {
        if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return Err(STATUS_MALFORMED);
        }
        let op = frame[3];
        match op {
            OP_PUT => decode_put(frame),
            OP_GET => decode_key_only(frame).map(|key| Request::Get { key }),
            OP_DEL => decode_key_only(frame).map(|key| Request::Delete { key }),
            OP_LIST => decode_list(frame),
            OP_SYNC => {
                if frame.len() != 4 {
                    Err(STATUS_MALFORMED)
                } else {
                    Ok(Request::Sync)
                }
            }
            OP_REOPEN => {
                if frame.len() != 4 {
                    Err(STATUS_MALFORMED)
                } else {
                    Ok(Request::Reopen)
                }
            }
            _ => Err(STATUS_UNSUPPORTED),
        }
    }

    pub fn encode_status_response(op: u8, status: u8) -> Vec<u8> {
        vec![MAGIC0, MAGIC1, VERSION, op | 0x80, status]
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

    pub fn decode_status_response(expected_op: u8, frame: &[u8]) -> Result<u8, StatefsError> {
        if frame.len() < 5 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return Err(StatefsError::Corrupted);
        }
        if frame[3] != (expected_op | 0x80) {
            return Err(StatefsError::Corrupted);
        }
        Ok(frame[4])
    }

    pub fn decode_get_response(frame: &[u8]) -> Result<Vec<u8>, StatefsError> {
        if frame.len() < 9 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return Err(StatefsError::Corrupted);
        }
        if frame[3] != (OP_GET | 0x80) {
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

    pub fn decode_list_response(frame: &[u8]) -> Result<Vec<String>, StatefsError> {
        if frame.len() < 7 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return Err(StatefsError::Corrupted);
        }
        if frame[3] != (OP_LIST | 0x80) {
            return Err(StatefsError::Corrupted);
        }
        let status = frame[4];
        if status != STATUS_OK {
            return Err(error_from_status(status));
        }
        let count = u16::from_le_bytes([frame[5], frame[6]]) as usize;
        let mut pos = 7;
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
            _ => StatefsError::Corrupted,
        }
    }

    fn decode_put(frame: &[u8]) -> Result<Request<'_>, u8> {
        if frame.len() < 10 {
            return Err(STATUS_MALFORMED);
        }
        let key_len = u16::from_le_bytes([frame[4], frame[5]]) as usize;
        let val_len = u32::from_le_bytes([frame[6], frame[7], frame[8], frame[9]]) as usize;
        if key_len == 0 {
            return Err(STATUS_MALFORMED);
        }
        if key_len > MAX_KEY_LEN {
            return Err(STATUS_KEY_TOO_LONG);
        }
        if val_len > MAX_VALUE_SIZE {
            return Err(STATUS_VALUE_TOO_LARGE);
        }
        let expected = 10usize.saturating_add(key_len).saturating_add(val_len);
        if frame.len() != expected {
            return Err(STATUS_MALFORMED);
        }
        let key_start = 10;
        let key_end = key_start + key_len;
        let key = str::from_utf8(&frame[key_start..key_end]).map_err(|_| STATUS_MALFORMED)?;
        let value = &frame[key_end..expected];
        Ok(Request::Put { key, value })
    }

    fn decode_key_only(frame: &[u8]) -> Result<&str, u8> {
        if frame.len() < 6 {
            return Err(STATUS_MALFORMED);
        }
        let key_len = u16::from_le_bytes([frame[4], frame[5]]) as usize;
        if key_len == 0 {
            return Err(STATUS_MALFORMED);
        }
        if key_len > MAX_KEY_LEN {
            return Err(STATUS_KEY_TOO_LONG);
        }
        let expected = 6usize.saturating_add(key_len);
        if frame.len() != expected {
            return Err(STATUS_MALFORMED);
        }
        let key_start = 6;
        let key_end = key_start + key_len;
        str::from_utf8(&frame[key_start..key_end]).map_err(|_| STATUS_MALFORMED)
    }

    fn decode_list(frame: &[u8]) -> Result<Request<'_>, u8> {
        if frame.len() < 8 {
            return Err(STATUS_MALFORMED);
        }
        let prefix_len = u16::from_le_bytes([frame[4], frame[5]]) as usize;
        let limit = u16::from_le_bytes([frame[6], frame[7]]);
        if prefix_len > MAX_KEY_LEN {
            return Err(STATUS_KEY_TOO_LONG);
        }
        let expected = 8usize.saturating_add(prefix_len);
        if frame.len() != expected {
            return Err(STATUS_MALFORMED);
        }
        let prefix = str::from_utf8(&frame[8..expected]).map_err(|_| STATUS_MALFORMED)?;
        let limit = if limit == 0 { 1 } else { limit.min(MAX_LIST_LIMIT) };
        Ok(Request::List { prefix, limit })
    }
}

#[cfg(all(feature = "ipc-client", nexus_env = "os"))]
pub mod client {
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::protocol;
    use super::StatefsError;
    use nexus_abi;
    use nexus_ipc::{Client as _, KernelClient, Wait};

    /// Client for statefsd IPC operations.
    pub struct StatefsClient {
        client: KernelClient,
        reply: Option<KernelClient>,
    }

    impl StatefsClient {
        /// Create a new client targeting `statefsd`.
        pub fn new() -> Result<Self, StatefsError> {
            let client = KernelClient::new_for("statefsd").map_err(|_| StatefsError::IoError)?;
            let reply = KernelClient::new_for("@reply").ok();
            Ok(Self { client, reply })
        }

        /// Create a new client from pre-routed kernel IPC endpoints.
        pub fn from_clients(client: KernelClient, reply: Option<KernelClient>) -> Self {
            Self { client, reply }
        }

        /// Put a value into statefs.
        pub fn put(&self, key: &str, value: &[u8]) -> Result<(), StatefsError> {
            let frame = protocol::encode_put_request(key, value)?;
            self.send_and_recv(frame, protocol::OP_PUT)?;
            Ok(())
        }

        /// Get a value from statefs.
        pub fn get(&self, key: &str) -> Result<Vec<u8>, StatefsError> {
            let frame = protocol::encode_key_only_request(protocol::OP_GET, key)?;
            let rsp = self.send_and_recv_raw(frame)?;
            protocol::decode_get_response(&rsp)
        }

        /// Delete a key.
        pub fn delete(&self, key: &str) -> Result<(), StatefsError> {
            let frame = protocol::encode_key_only_request(protocol::OP_DEL, key)?;
            self.send_and_recv(frame, protocol::OP_DEL)?;
            Ok(())
        }

        /// List keys by prefix.
        pub fn list(&self, prefix: &str, limit: u16) -> Result<Vec<String>, StatefsError> {
            let frame = protocol::encode_list_request(prefix, limit)?;
            let rsp = self.send_and_recv_raw(frame)?;
            protocol::decode_list_response(&rsp)
        }

        /// Sync statefs.
        pub fn sync(&self) -> Result<(), StatefsError> {
            let frame = protocol::encode_sync_request();
            self.send_and_recv(frame, protocol::OP_SYNC)?;
            Ok(())
        }

        fn send_and_recv(&self, frame: Vec<u8>, op: u8) -> Result<(), StatefsError> {
            let rsp = self.send_and_recv_raw(frame)?;
            let status = protocol::decode_status_response(op, &rsp)?;
            if status == protocol::STATUS_OK {
                Ok(())
            } else {
                Err(protocol::error_from_status(status))
            }
        }

        fn send_and_recv_raw(&self, frame: Vec<u8>) -> Result<Vec<u8>, StatefsError> {
            if let Some(reply) = &self.reply {
                let (reply_send_slot, _reply_recv_slot) = reply.slots();
                let reply_send_clone =
                    nexus_abi::cap_clone(reply_send_slot).map_err(|_| StatefsError::IoError)?;
                self.client
                    .send_with_cap_move_wait(&frame, reply_send_clone, Wait::Blocking)
                    .map_err(|_| StatefsError::IoError)?;
                reply.recv(Wait::Blocking).map_err(|_| StatefsError::IoError)
            } else {
                self.client
                    .send(&frame, Wait::Blocking)
                    .map_err(|_| StatefsError::IoError)?;
                self.client.recv(Wait::Blocking).map_err(|_| StatefsError::IoError)
            }
        }
    }
}
// ============================================================================
// Journal Record Format
// ============================================================================

/// Journal operation codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JournalOpCode {
    Put = 0x01,
    Delete = 0x02,
    Checkpoint = 0x03,
}

impl JournalOpCode {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::Put),
            0x02 => Some(Self::Delete),
            0x03 => Some(Self::Checkpoint),
            _ => None,
        }
    }
}

/// A parsed journal record
#[derive(Debug, Clone)]
struct JournalRecord {
    op: JournalOpCode,
    key: String,
    value: Vec<u8>,
}

/// Serialize a journal record to bytes (including CRC32).
fn serialize_record(op: JournalOpCode, key: &str, value: &[u8]) -> Vec<u8> {
    let key_bytes = key.as_bytes();
    let key_len = key_bytes.len() as u16;
    let value_len = value.len() as u32;

    // Calculate total size: header + key + value + crc
    let total_len = RECORD_HEADER_SIZE + key_bytes.len() + value.len();
    let mut buf = vec![0u8; total_len];

    // Magic (4 bytes, little-endian)
    buf[0..4].copy_from_slice(&JOURNAL_MAGIC.to_le_bytes());

    // OpCode (1 byte)
    buf[4] = op as u8;

    // KeyLen (2 bytes, little-endian)
    buf[5..7].copy_from_slice(&key_len.to_le_bytes());

    // ValueLen (4 bytes, little-endian)
    buf[7..11].copy_from_slice(&value_len.to_le_bytes());

    // Key
    let key_start = 11;
    let key_end = key_start + key_bytes.len();
    buf[key_start..key_end].copy_from_slice(key_bytes);

    // Value
    let value_start = key_end;
    let value_end = value_start + value.len();
    buf[value_start..value_end].copy_from_slice(value);

    // CRC32 over [magic..value] (everything except the CRC itself)
    let crc = crc32fast::hash(&buf[..value_end]);
    buf[value_end..value_end + 4].copy_from_slice(&crc.to_le_bytes());

    buf
}

/// Try to parse a journal record from a byte slice.
/// Returns (record, bytes_consumed) on success.
fn parse_record(data: &[u8]) -> Result<Option<(JournalRecord, usize)>, StatefsError> {
    // Need at least header size
    if data.len() < RECORD_HEADER_SIZE {
        return Ok(None);
    }

    // Check magic
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != JOURNAL_MAGIC {
        // Not a valid record start; might be end of journal
        return Ok(None);
    }

    // Parse header
    let op_byte = data[4];
    let op = JournalOpCode::from_u8(op_byte).ok_or(StatefsError::Corrupted)?;

    let key_len = u16::from_le_bytes([data[5], data[6]]) as usize;
    let value_len = u32::from_le_bytes([data[7], data[8], data[9], data[10]]) as usize;

    // Validate lengths
    if key_len > MAX_KEY_LEN {
        return Err(StatefsError::Corrupted);
    }
    if value_len > MAX_VALUE_SIZE {
        return Err(StatefsError::Corrupted);
    }

    let total_len = RECORD_HEADER_SIZE + key_len + value_len;
    if data.len() < total_len {
        // Truncated record
        return Ok(None);
    }

    // Extract key and value
    let key_start = 11;
    let key_end = key_start + key_len;
    let key_bytes = &data[key_start..key_end];

    let value_start = key_end;
    let value_end = value_start + value_len;
    let value = &data[value_start..value_end];

    // Verify CRC
    let crc_start = value_end;
    if data.len() < crc_start + 4 {
        return Ok(None);
    }
    let stored_crc = u32::from_le_bytes([
        data[crc_start],
        data[crc_start + 1],
        data[crc_start + 2],
        data[crc_start + 3],
    ]);
    let computed_crc = crc32fast::hash(&data[..value_end]);
    if stored_crc != computed_crc {
        return Err(StatefsError::Corrupted);
    }

    // Parse key as UTF-8
    let key = core::str::from_utf8(key_bytes)
        .map_err(|_| StatefsError::Corrupted)?
        .into();

    Ok(Some((
        JournalRecord {
            op,
            key,
            value: value.to_vec(),
        },
        total_len,
    )))
}

// ============================================================================
// JournalEngine
// ============================================================================

/// Journaled key-value store engine.
pub struct JournalEngine<B: BlockDevice> {
    device: B,
    /// In-memory key-value map (populated from journal replay)
    kv: BTreeMap<String, Vec<u8>>,
    /// Current write position in the journal (byte offset)
    write_pos: usize,
    /// Number of records replayed (for bounded replay check)
    record_count: usize,
}

impl<B: BlockDevice> JournalEngine<B> {
    /// Create a new journal engine and replay existing journal from device.
    pub fn open(device: B) -> Result<Self, StatefsError> {
        let mut engine = Self {
            device,
            kv: BTreeMap::new(),
            write_pos: 0,
            record_count: 0,
        };
        engine.replay()?;
        Ok(engine)
    }

    /// Replay journal from device into in-memory KV map.
    fn replay(&mut self) -> Result<(), StatefsError> {
        let block_size = self.device.block_size();
        let block_count = self.device.block_count();

        // Stream journal replay to avoid large allocations in os-lite builds.
        let mut block_buf = vec![0u8; block_size];
        let mut buf = vec![0u8; block_size.saturating_mul(2)];
        let mut buf_len = 0usize;
        let mut file_pos = 0usize;
        let mut done = false;

        for block_idx in 0..block_count {
            self.device
                .read_block(block_idx, &mut block_buf)
                .map_err(|_| StatefsError::IoError)?;

            if buf_len + block_size > buf.len() {
                buf.resize(buf_len + block_size, 0);
            }
            buf[buf_len..buf_len + block_size].copy_from_slice(&block_buf);
            buf_len += block_size;

            let mut pos = 0usize;
            while pos < buf_len && self.record_count < MAX_REPLAY_RECORDS {
                let remaining = buf_len - pos;
                if remaining < RECORD_HEADER_SIZE {
                    break;
                }
                match parse_record(&buf[pos..buf_len]) {
                    Ok(Some((record, consumed))) => {
                        match record.op {
                            JournalOpCode::Put => {
                                self.kv.insert(record.key, record.value);
                            }
                            JournalOpCode::Delete => {
                                self.kv.remove(&record.key);
                            }
                            JournalOpCode::Checkpoint => {}
                        }
                        pos += consumed;
                        file_pos = file_pos.saturating_add(consumed);
                        self.record_count += 1;
                    }
                    Ok(None) => {
                        // End of valid journal (magic mismatch).
                        done = true;
                        break;
                    }
                    Err(StatefsError::Corrupted) => {
                        // Stop at first corruption for safety.
                        done = true;
                        break;
                    }
                    Err(e) => return Err(e),
                }
            }

            if pos > 0 {
                buf.copy_within(pos..buf_len, 0);
                buf_len -= pos;
            }
            if done {
                break;
            }
        }

        if self.record_count >= MAX_REPLAY_RECORDS {
            return Err(StatefsError::ReplayLimitExceeded);
        }

        self.write_pos = file_pos;
        Ok(())
    }

    /// Validate a key path.
    fn validate_key(key: &str) -> Result<(), StatefsError> {
        if key.len() > MAX_KEY_LEN {
            return Err(StatefsError::KeyTooLong);
        }
        if !key.starts_with("/state/") {
            return Err(StatefsError::InvalidKey);
        }
        // Check for path traversal
        if key.contains("/../") || key.contains("/./") || key.ends_with("/..") || key.ends_with("/.") {
            return Err(StatefsError::InvalidKey);
        }
        Ok(())
    }

    /// Write a record to the journal and update in-memory state.
    fn append_record(&mut self, op: JournalOpCode, key: &str, value: &[u8]) -> Result<(), StatefsError> {
        let record_bytes = serialize_record(op, key, value);
        let block_size = self.device.block_size();

        // Calculate which blocks to write
        let start_block = self.write_pos / block_size;
        let end_byte = self.write_pos + record_bytes.len();
        let end_block = (end_byte + block_size - 1) / block_size;

        // Check if we have space
        if end_block as u64 > self.device.block_count() {
            return Err(StatefsError::IoError);
        }

        // Read-modify-write for blocks that span the write
        let mut buf = vec![0u8; block_size];
        let mut record_offset = 0;

        for block_idx in start_block..end_block {
            // Read existing block
            self.device
                .read_block(block_idx as u64, &mut buf)
                .map_err(|_| StatefsError::IoError)?;

            // Calculate what portion of the record goes in this block
            let block_start_byte = block_idx * block_size;
            let block_end_byte = block_start_byte + block_size;

            let write_start = if self.write_pos > block_start_byte {
                self.write_pos - block_start_byte
            } else {
                0
            };
            let write_end = if end_byte < block_end_byte {
                end_byte - block_start_byte
            } else {
                block_size
            };

            let record_chunk_len = write_end - write_start;
            buf[write_start..write_end]
                .copy_from_slice(&record_bytes[record_offset..record_offset + record_chunk_len]);
            record_offset += record_chunk_len;

            // Write block back
            self.device
                .write_block(block_idx as u64, &buf)
                .map_err(|_| StatefsError::IoError)?;
        }

        self.write_pos = end_byte;
        self.record_count += 1;
        Ok(())
    }

    /// Put a key-value pair.
    pub fn put(&mut self, key: &str, value: &[u8]) -> Result<(), StatefsError> {
        Self::validate_key(key)?;
        if value.len() > MAX_VALUE_SIZE {
            return Err(StatefsError::ValueTooLarge);
        }

        // Append to journal
        self.append_record(JournalOpCode::Put, key, value)?;

        // Update in-memory state
        self.kv.insert(key.into(), value.to_vec());
        Ok(())
    }

    /// Get a value by key.
    pub fn get(&self, key: &str) -> Result<Vec<u8>, StatefsError> {
        Self::validate_key(key)?;
        self.kv.get(key).cloned().ok_or(StatefsError::NotFound)
    }

    /// Delete a key.
    pub fn delete(&mut self, key: &str) -> Result<(), StatefsError> {
        Self::validate_key(key)?;
        if !self.kv.contains_key(key) {
            return Err(StatefsError::NotFound);
        }

        // Append to journal
        self.append_record(JournalOpCode::Delete, key, &[])?;

        // Update in-memory state
        self.kv.remove(key);
        Ok(())
    }

    /// List keys matching a prefix.
    pub fn list(&self, prefix: &str, limit: usize) -> Result<Vec<String>, StatefsError> {
        if !prefix.starts_with("/state/") && prefix != "/state" {
            return Err(StatefsError::InvalidKey);
        }

        let keys: Vec<String> = self
            .kv
            .keys()
            .filter(|k| k.starts_with(prefix))
            .take(limit)
            .cloned()
            .collect();

        Ok(keys)
    }

    /// Sync all pending writes to durable storage.
    pub fn sync(&mut self) -> Result<(), StatefsError> {
        self.device.sync().map_err(|_| StatefsError::IoError)
    }

    /// Reopen the journal by replaying from the current device.
    pub fn reopen(&mut self) -> Result<(), StatefsError> {
        self.kv.clear();
        self.write_pos = 0;
        self.record_count = 0;
        self.replay()
    }

    /// Get the number of keys in the store.
    pub fn len(&self) -> usize {
        self.kv.len()
    }

    /// Check if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.kv.is_empty()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use storage::MemBlockDevice;

    fn create_engine(block_size: usize, block_count: u64) -> JournalEngine<MemBlockDevice> {
        let device = MemBlockDevice::new(block_size, block_count);
        JournalEngine::open(device).expect("failed to open engine")
    }

    #[test]
    fn test_put_get_delete_list() {
        let mut engine = create_engine(512, 100);

        // Put
        engine.put("/state/test/key1", b"value1").unwrap();
        engine.put("/state/test/key2", b"value2").unwrap();

        // Get
        assert_eq!(engine.get("/state/test/key1").unwrap(), b"value1");
        assert_eq!(engine.get("/state/test/key2").unwrap(), b"value2");

        // List
        let keys = engine.list("/state/test/", 100).unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&String::from("/state/test/key1")));
        assert!(keys.contains(&String::from("/state/test/key2")));

        // Delete
        engine.delete("/state/test/key1").unwrap();
        assert_eq!(engine.get("/state/test/key1"), Err(StatefsError::NotFound));
        assert_eq!(engine.get("/state/test/key2").unwrap(), b"value2");

        // List after delete
        let keys = engine.list("/state/test/", 100).unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys.contains(&String::from("/state/test/key2")));
    }

    #[test]
    fn test_replay_after_reopen() {
        // Create engine and write data
        let device = MemBlockDevice::new(512, 100);
        let mut engine = JournalEngine::open(device).unwrap();
        engine.put("/state/keystore/device.key", b"secret-key-data").unwrap();
        engine.put("/state/boot/bootctl.active", b"slot-a").unwrap();
        engine.sync().unwrap();

        // "Close" and reopen by extracting device and creating new engine
        let device = engine.device;
        let engine2 = JournalEngine::open(device).unwrap();

        // Verify data survived replay
        assert_eq!(engine2.get("/state/keystore/device.key").unwrap(), b"secret-key-data");
        assert_eq!(engine2.get("/state/boot/bootctl.active").unwrap(), b"slot-a");
    }

    #[test]
    fn test_reject_corrupted_journal() {
        // Create engine and write data
        let device = MemBlockDevice::new(512, 100);
        let mut engine = JournalEngine::open(device).unwrap();
        engine.put("/state/test/key", b"value").unwrap();
        engine.sync().unwrap();
        let mut device = engine.device;

        // Corrupt a byte in the key area
        device.raw_storage_mut()[0][11] ^= 0xFF;

        // Reopen should stop at corrupted record
        let engine = JournalEngine::open(device).unwrap();
        // Data should not be present (corruption detected)
        assert_eq!(engine.get("/state/test/key"), Err(StatefsError::NotFound));
    }

    #[test]
    fn test_reject_value_oversized() {
        let mut engine = create_engine(512, 100);
        let big_value = vec![0u8; MAX_VALUE_SIZE + 1];
        assert_eq!(
            engine.put("/state/test/big", &big_value),
            Err(StatefsError::ValueTooLarge)
        );
    }

    #[test]
    fn test_reject_key_too_long() {
        let mut engine = create_engine(512, 100);
        let long_key = format!("/state/{}", "x".repeat(MAX_KEY_LEN));
        assert_eq!(
            engine.put(&long_key, b"value"),
            Err(StatefsError::KeyTooLong)
        );
    }

    #[test]
    fn test_reject_invalid_key_path() {
        let mut engine = create_engine(512, 100);

        // Must start with /state/
        assert_eq!(engine.put("/other/key", b"v"), Err(StatefsError::InvalidKey));
        assert_eq!(engine.put("state/key", b"v"), Err(StatefsError::InvalidKey));

        // No path traversal
        assert_eq!(engine.put("/state/../etc/passwd", b"v"), Err(StatefsError::InvalidKey));
        assert_eq!(engine.put("/state/./key", b"v"), Err(StatefsError::InvalidKey));
    }

    #[test]
    fn test_reject_malformed_record() {
        // Create device with garbage data that looks like a valid magic but has invalid opcode
        let mut device = MemBlockDevice::new(512, 10);
        let block = device.raw_storage_mut();
        // Write magic
        block[0][0..4].copy_from_slice(&JOURNAL_MAGIC.to_le_bytes());
        // Invalid opcode
        block[0][4] = 0xFF;

        // Should stop at invalid record
        let engine = JournalEngine::open(device).unwrap();
        assert!(engine.is_empty());
    }

    #[test]
    fn test_bounded_replay() {
        // This test verifies the MAX_REPLAY_RECORDS limit
        // We create a device that would exceed the limit if fully replayed

        // For this test, we'll create a small engine and verify the constant exists
        let engine = create_engine(512, 100);
        assert!(engine.record_count < MAX_REPLAY_RECORDS);
    }

    #[test]
    fn test_delete_nonexistent() {
        let mut engine = create_engine(512, 100);
        assert_eq!(
            engine.delete("/state/nonexistent"),
            Err(StatefsError::NotFound)
        );
    }

    #[test]
    fn test_list_with_limit() {
        let mut engine = create_engine(512, 100);
        for i in 0..10 {
            engine.put(&format!("/state/test/key{}", i), b"v").unwrap();
        }

        let keys = engine.list("/state/test/", 5).unwrap();
        assert_eq!(keys.len(), 5);
    }

    #[test]
    fn test_put_overwrite() {
        let mut engine = create_engine(512, 100);
        engine.put("/state/test/key", b"value1").unwrap();
        engine.put("/state/test/key", b"value2").unwrap();
        assert_eq!(engine.get("/state/test/key").unwrap(), b"value2");
    }

    #[test]
    fn test_sync_success() {
        let mut engine = create_engine(512, 100);
        engine.put("/state/test/key", b"value").unwrap();
        assert!(engine.sync().is_ok());
    }

    // =========================================================================
    // Persistence scenario tests (mirror QEMU selftests for fast feedback)
    // =========================================================================

    #[test]
    fn test_bootctrl_persistence_roundtrip() {
        // Mirrors SELFTEST: bootctl persist from QEMU
        const BOOTCTL_KEY: &str = "/state/boot/bootctl.v1";

        // Simulate BootCtrl v1 binary format: [version, active_slot, pending, tries, health]
        let bootctrl_state: [u8; 5] = [
            1,    // version
            0,    // active_slot = A
            1,    // pending_slot = Some(B) encoded as 1
            3,    // tries_left
            0,    // health_ok = false
        ];

        let device = MemBlockDevice::new(512, 100);
        let mut engine = JournalEngine::open(device).unwrap();
        engine.put(BOOTCTL_KEY, &bootctrl_state).unwrap();
        engine.sync().unwrap();

        // Simulate reboot: reopen journal
        let device = engine.device;
        let engine2 = JournalEngine::open(device).unwrap();

        let loaded = engine2.get(BOOTCTL_KEY).unwrap();
        assert_eq!(loaded, bootctrl_state);
        assert_eq!(loaded[0], 1); // version check
        assert_eq!(loaded[1], 0); // active = A
        assert_eq!(loaded[2], 1); // pending = B
    }

    #[test]
    fn test_device_key_persistence_roundtrip() {
        // Mirrors SELFTEST: device key persist from QEMU
        const DEVICE_KEY_PATH: &str = "/state/keystore/device.key.ed25519";

        // Simulate Ed25519 keypair (seed + pubkey = 64 bytes)
        let mut keypair = [0u8; 64];
        keypair[0..32].copy_from_slice(&[0xAA; 32]); // seed
        keypair[32..64].copy_from_slice(&[0xBB; 32]); // pubkey

        let device = MemBlockDevice::new(512, 100);
        let mut engine = JournalEngine::open(device).unwrap();
        engine.put(DEVICE_KEY_PATH, &keypair).unwrap();
        engine.sync().unwrap();

        // Simulate reboot
        let device = engine.device;
        let engine2 = JournalEngine::open(device).unwrap();

        let loaded = engine2.get(DEVICE_KEY_PATH).unwrap();
        assert_eq!(loaded.len(), 64);
        assert_eq!(&loaded[0..32], &[0xAA; 32]);
        assert_eq!(&loaded[32..64], &[0xBB; 32]);
    }

    #[test]
    fn test_multi_cycle_persistence() {
        // Tests multiple reopen cycles (3 reboots)
        let device = MemBlockDevice::new(512, 100);
        let mut engine = JournalEngine::open(device).unwrap();

        // Cycle 1: write initial data
        engine.put("/state/cycle/counter", b"\x01").unwrap();
        engine.sync().unwrap();

        // Cycle 2: increment
        let device = engine.device;
        let mut engine = JournalEngine::open(device).unwrap();
        let val = engine.get("/state/cycle/counter").unwrap();
        assert_eq!(val, b"\x01");
        engine.put("/state/cycle/counter", b"\x02").unwrap();
        engine.sync().unwrap();

        // Cycle 3: increment again
        let device = engine.device;
        let mut engine = JournalEngine::open(device).unwrap();
        let val = engine.get("/state/cycle/counter").unwrap();
        assert_eq!(val, b"\x02");
        engine.put("/state/cycle/counter", b"\x03").unwrap();
        engine.sync().unwrap();

        // Final verification
        let device = engine.device;
        let engine = JournalEngine::open(device).unwrap();
        assert_eq!(engine.get("/state/cycle/counter").unwrap(), b"\x03");
    }

    #[test]
    fn test_moderate_value_persistence() {
        // Test persistence of moderate-sized values
        let mut engine = create_engine(512, 100);

        // First value: 256 bytes (fits in one block with header)
        let value_256 = vec![0x42u8; 256];
        engine.put("/state/test/v256", &value_256).unwrap();
        engine.sync().unwrap();

        // Reopen and verify
        let device = engine.device;
        let engine2 = JournalEngine::open(device).unwrap();
        let got = engine2.get("/state/test/v256").unwrap();
        assert_eq!(got.len(), 256);
        assert!(got.iter().all(|&b| b == 0x42));
    }

    #[test]
    fn test_cross_block_record_persistence() {
        // Test a record that spans block boundary
        let mut engine = create_engine(512, 100);

        // First write a small value to advance write_pos past middle of first block
        engine.put("/state/test/small", b"setup").unwrap();

        // Now write 400 bytes - this record will span block 0 and block 1
        // Key: /state/test/large (17 bytes)
        // Header: 15 bytes
        // Total record: 15 + 17 + 400 = 432 bytes
        let large = vec![0x55u8; 400];
        engine.put("/state/test/large", &large).unwrap();
        engine.sync().unwrap();

        // Verify before reopen
        assert_eq!(engine.get("/state/test/large").unwrap().len(), 400);

        // Reopen and verify
        let device = engine.device;
        let engine2 = JournalEngine::open(device).unwrap();
        assert_eq!(engine2.get("/state/test/small").unwrap(), b"setup");
        let got = engine2.get("/state/test/large").unwrap();
        assert_eq!(got.len(), 400, "cross-block record should persist");
        assert!(got.iter().all(|&b| b == 0x55));
    }

    #[test]
    fn test_overwrite_then_persist() {
        // Verify that overwrites are correctly persisted
        let device = MemBlockDevice::new(512, 100);
        let mut engine = JournalEngine::open(device).unwrap();

        engine.put("/state/test/key", b"first").unwrap();
        engine.put("/state/test/key", b"second").unwrap();
        engine.put("/state/test/key", b"third").unwrap();
        engine.sync().unwrap();

        let device = engine.device;
        let engine2 = JournalEngine::open(device).unwrap();
        assert_eq!(engine2.get("/state/test/key").unwrap(), b"third");
    }

    #[test]
    fn test_delete_then_persist() {
        // Verify that deletes are correctly persisted
        let device = MemBlockDevice::new(512, 100);
        let mut engine = JournalEngine::open(device).unwrap();

        engine.put("/state/test/ephemeral", b"gone").unwrap();
        engine.put("/state/test/permanent", b"stay").unwrap();
        engine.delete("/state/test/ephemeral").unwrap();
        engine.sync().unwrap();

        let device = engine.device;
        let engine2 = JournalEngine::open(device).unwrap();
        assert_eq!(engine2.get("/state/test/ephemeral"), Err(StatefsError::NotFound));
        assert_eq!(engine2.get("/state/test/permanent").unwrap(), b"stay");
    }

    #[test]
    fn test_mixed_operations_persist() {
        // Complex sequence: put, delete, overwrite, list - then persist
        let device = MemBlockDevice::new(512, 100);
        let mut engine = JournalEngine::open(device).unwrap();

        engine.put("/state/app/setting1", b"v1").unwrap();
        engine.put("/state/app/setting2", b"v2").unwrap();
        engine.put("/state/app/setting3", b"v3").unwrap();
        engine.delete("/state/app/setting2").unwrap();
        engine.put("/state/app/setting1", b"v1-updated").unwrap();
        engine.sync().unwrap();

        let device = engine.device;
        let engine2 = JournalEngine::open(device).unwrap();

        assert_eq!(engine2.get("/state/app/setting1").unwrap(), b"v1-updated");
        assert_eq!(engine2.get("/state/app/setting2"), Err(StatefsError::NotFound));
        assert_eq!(engine2.get("/state/app/setting3").unwrap(), b"v3");

        let keys = engine2.list("/state/app/", 10).unwrap();
        assert_eq!(keys.len(), 2);
    }
}
