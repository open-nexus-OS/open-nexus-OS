// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS journaled key-value store for /state persistence
//! OWNERS: @runtime
//! STATUS: Functional (host-first)
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: Host unit tests + negative tests
//!
//! PUBLIC API:
//!   - JournalEngine: Journaled KV store with Put/Get/Delete/List/Sync, plus
//!     journal v2 (TASK-0026): txn_begin/append/commit/abort, sync_barrier,
//!     maybe_compact/compact_now (see journal_v2.rs / compact.rs)
//!   - fsck: offline validate/repair core for tools/fsck-statefs (fsck.rs)
//!   - protocol: IPC framing helpers for statefsd
//!   - client: IPC client wrapper (feature = "ipc-client")
//!   - envelope: Authenticity envelope v1 (seal/verify, SeqTracker, WriteBudget)
//!   - StatefsError: Error types
//!
//! DEPENDENCIES:
//!   - crc32c: CRC32-C checksums for journal integrity
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
pub(crate) const JOURNAL_MAGIC: u32 = 0x4E58_5346;

/// Maximum key length in bytes
pub const MAX_KEY_LEN: usize = 255;

/// Maximum value size in bytes (64 KiB)
pub const MAX_VALUE_SIZE: usize = 65536;

/// Maximum records to replay (bounded replay)
const MAX_REPLAY_RECORDS: usize = 100_000;

/// Journal record header size: magic(4) + opcode(1) + key_len(2) + value_len(4) + crc(4) = 15
pub(crate) const RECORD_HEADER_SIZE: usize = 15;

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
    /// Envelope authenticity check failed (tamper / wrong key / downgrade)
    IntegrityViolation,
    /// Stale sequence number for a key (anti-rollback)
    RollbackDetected,
}

impl StatefsError {
    /// Stable no-alloc diagnostic label (for UART/audit lines in no_std
    /// consumers; not a wire format — statuses stay in `protocol`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::NotFound => "NotFound",
            Self::AccessDenied => "AccessDenied",
            Self::ValueTooLarge => "ValueTooLarge",
            Self::KeyTooLong => "KeyTooLong",
            Self::IoError => "IoError",
            Self::Corrupted => "Corrupted",
            Self::InvalidKey => "InvalidKey",
            Self::ReplayLimitExceeded => "ReplayLimitExceeded",
            Self::IntegrityViolation => "IntegrityViolation",
            Self::RollbackDetected => "RollbackDetected",
        }
    }
}

// ============================================================================
// IPC Protocol (statefsd)
// ============================================================================

pub mod protocol;

#[cfg(all(feature = "ipc-client", nexus_env = "os"))]
pub mod client;
pub mod compact;
pub mod derive;
pub mod envelope;
pub mod fsck;
pub mod journal_v2;
pub mod record;
pub mod writer;

pub use compact::{CompactionConfig, CompactionStats};
pub use fsck::{fsck, FsckFault, FsckOutcome, FsckReport, JournalLayout, FSCK_BLOCK_SIZE};
pub use journal_v2::JournalOpCode;

use journal_v2::TxnTable;

// ============================================================================
// Journal Record Format (byte layouts: journal_v2.rs; framing is frozen v1)
// ============================================================================

/// Compute CRC32-C (Castagnoli) over data.
pub(crate) fn crc32c(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0x82F63B78 & mask);
        }
    }
    !crc
}

// ============================================================================
// JournalEngine
// ============================================================================

/// Journaled key-value store engine.
///
/// Layout on the device (detected at open, see `compact::detect_layout`):
/// - legacy v1: records from block 0 over the whole device (generation 0);
/// - v2 (post-compaction): block 0 = `NXS2` superblock, records in the
///   active half of an A/B region split (generation >= 1).
pub struct JournalEngine<B: BlockDevice> {
    device: B,
    /// In-memory key-value map (populated from journal replay)
    kv: BTreeMap<String, Vec<u8>>,
    /// Current write position, in bytes RELATIVE to the region start
    write_pos: usize,
    /// Number of records replayed/appended (for bounded replay check)
    record_count: usize,
    /// Records scanned by the most recent replay (incremental-replay proof)
    replayed_count: usize,
    /// Anomalies (orphaned txn records) seen by the most recent replay
    replay_orphans: usize,
    /// First device block of the live journal region (0 = legacy layout)
    region_first_block: u64,
    /// Number of blocks in the live journal region
    region_block_count: u64,
    /// Journal generation (0 until the first compaction)
    generation: u32,
    /// Next transaction id (max id ever seen + 1; ids are never reused)
    next_txn: u64,
    /// Open (prepared, uncommitted) transactions — bounded, see journal_v2
    txns: TxnTable,
    /// Compaction thresholds and per-cycle work bounds
    compaction: CompactionConfig,
    /// Persistent snapshot serialization buffer (capacity reused across
    /// compaction cycles — the os-lite bump allocator never frees, so
    /// per-cycle buffers must not be reallocated; see compact.rs)
    pub(crate) compact_scratch: Vec<u8>,
    /// Persistent one-block scratch: append RMW, compaction writes, and
    /// the readback verify all reuse it (never a fresh Vec per append)
    pub(crate) block_scratch: Vec<u8>,
    /// Persistent record-encoding scratch for `append_record`
    record_scratch: Vec<u8>,
}

impl<B: BlockDevice> JournalEngine<B> {
    /// Create a new journal engine and replay existing journal from device.
    pub fn open(device: B) -> Result<Self, StatefsError> {
        let mut engine = Self {
            device,
            kv: BTreeMap::new(),
            write_pos: 0,
            record_count: 0,
            replayed_count: 0,
            replay_orphans: 0,
            region_first_block: 0,
            region_block_count: 0,
            generation: 0,
            next_txn: 1,
            txns: TxnTable::new(),
            compaction: CompactionConfig::default(),
            compact_scratch: Vec::new(),
            block_scratch: Vec::new(),
            record_scratch: Vec::new(),
        };
        engine.detect_layout()?;
        engine.replay()?;
        Ok(engine)
    }

    /// Replay the live journal region into the in-memory KV map.
    /// Committed-only 2PC semantics live in `journal_v2::Replayer`; this
    /// loop only streams region blocks and frames records.
    fn replay(&mut self) -> Result<(), StatefsError> {
        let block_size = self.device.block_size();
        let block_count = self.region_block_count;

        // Stream journal replay to avoid large allocations in os-lite builds.
        let mut block_buf = vec![0u8; block_size];
        let mut buf = vec![0u8; block_size.saturating_mul(2)];
        let mut buf_len = 0usize;
        let mut file_pos = 0usize;
        let mut done = false;
        let mut replayer = journal_v2::Replayer::new();

        for block_idx in 0..block_count {
            self.device
                .read_block(self.region_first_block + block_idx, &mut block_buf)
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
                // If we have magic but not enough bytes for a full record yet, wait for more data.
                if buf[pos..pos + 4] == JOURNAL_MAGIC.to_le_bytes() {
                    let key_len = u16::from_le_bytes([buf[pos + 5], buf[pos + 6]]) as usize;
                    let value_len = u32::from_le_bytes([
                        buf[pos + 7],
                        buf[pos + 8],
                        buf[pos + 9],
                        buf[pos + 10],
                    ]) as usize;
                    let total_len = RECORD_HEADER_SIZE + key_len + value_len;
                    if remaining < total_len {
                        break;
                    }
                }
                match journal_v2::parse_record(&buf[pos..buf_len]) {
                    Ok(Some((record, consumed))) => {
                        replayer.apply(&mut self.kv, record);
                        pos += consumed;
                        file_pos = file_pos.saturating_add(consumed);
                        self.record_count += 1;
                    }
                    Ok(None) => {
                        // End of valid journal (magic mismatch or truncated tail).
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

        // Prepared-without-commit tails are discarded here; discarded txn
        // ids stay burned (never reused).
        let outcome = replayer.finish();
        self.next_txn = core::cmp::max(self.next_txn, outcome.next_txn);
        // Superblock generation is the SSOT; the checkpoint record inside
        // the region carries the same number (fsck cross-check).
        self.generation = core::cmp::max(self.generation, outcome.generation);
        self.replay_orphans = outcome.orphans;
        self.replayed_count = self.record_count;
        self.write_pos = file_pos;
        Ok(())
    }

    /// Anomalies (orphaned/ignored transaction records) seen by the most
    /// recent replay. Informational — feeds the future `fsck-statefs`
    /// orphan detection; a nonzero count is not an error.
    pub fn replay_orphans(&self) -> usize {
        self.replay_orphans
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
        if key.contains("/../")
            || key.contains("/./")
            || key.ends_with("/..")
            || key.ends_with("/.")
        {
            return Err(StatefsError::InvalidKey);
        }
        Ok(())
    }

    /// Write a serialized record to the journal (region-relative append).
    ///
    /// Zeroed-tail invariant: the bytes past the new write head, through the
    /// end of the FOLLOWING block, are always zero — so replay stops exactly
    /// at the head and stale records from an earlier generation of a reused
    /// region can never be resurrected. Bounded: at most one extra block
    /// write per append (only when a record ends exactly on a block edge).
    pub(crate) fn append_bytes(&mut self, record_bytes: &[u8]) -> Result<(), StatefsError> {
        let block_size = self.device.block_size();

        // Calculate which region blocks to write
        let start_block = self.write_pos / block_size;
        let end_byte = self.write_pos + record_bytes.len();
        let end_block = end_byte.div_ceil(block_size);

        // Check if we have space in the live region
        if end_block as u64 > self.region_block_count {
            return Err(StatefsError::IoError);
        }

        // Read-modify-write for blocks that span the write. The one-block
        // buffer is the engine's persistent scratch — appends are the
        // hottest path, and the os-lite bump allocator never frees, so a
        // fresh Vec per append leaks the heap dry (an error `?` below
        // drops it; the next append re-allocates once).
        let mut buf = core::mem::take(&mut self.block_scratch);
        buf.clear();
        buf.resize(block_size, 0);
        let mut record_offset = 0;

        for block_idx in start_block..end_block {
            let device_block = self.region_first_block + block_idx as u64;
            // Read existing block
            self.device.read_block(device_block, &mut buf).map_err(|_| StatefsError::IoError)?;

            // Calculate what portion of the record goes in this block
            let block_start_byte = block_idx * block_size;
            let block_end_byte = block_start_byte + block_size;

            let write_start = self.write_pos.saturating_sub(block_start_byte);
            let write_end =
                if end_byte < block_end_byte { end_byte - block_start_byte } else { block_size };

            let record_chunk_len = write_end - write_start;
            buf[write_start..write_end]
                .copy_from_slice(&record_bytes[record_offset..record_offset + record_chunk_len]);
            record_offset += record_chunk_len;

            // Zeroed-tail: scrub stale bytes after the record in its last block.
            if block_idx + 1 == end_block && write_end < block_size {
                buf[write_end..].fill(0);
            }

            // Write block back
            self.device.write_block(device_block, &buf).map_err(|_| StatefsError::IoError)?;
        }

        // Record ends exactly on a block edge: zero the next block so the
        // head is always followed by non-record bytes.
        if end_byte % block_size == 0 && (end_block as u64) < self.region_block_count {
            buf.fill(0);
            self.device
                .write_block(self.region_first_block + end_block as u64, &buf)
                .map_err(|_| StatefsError::IoError)?;
        }

        self.write_pos = end_byte;
        self.record_count += 1;
        self.block_scratch = buf;
        Ok(())
    }

    /// Serialize and append one record (persistent encoding scratch — no
    /// per-record allocation on the hot path).
    fn append_record(
        &mut self,
        op: JournalOpCode,
        key: &str,
        value: &[u8],
    ) -> Result<(), StatefsError> {
        let mut record_bytes = core::mem::take(&mut self.record_scratch);
        record_bytes.clear();
        record::encode_record_into(&mut record_bytes, op, key, value);
        let result = self.append_bytes(&record_bytes);
        self.record_scratch = record_bytes;
        result
    }

    /// Put a key-value pair.
    pub fn put(&mut self, key: &str, value: &[u8]) -> Result<(), StatefsError> {
        Self::validate_key(key)?;
        if value.len() > MAX_VALUE_SIZE {
            return Err(StatefsError::ValueTooLarge);
        }

        // Append to journal
        self.append_record(JournalOpCode::Put, key, value)?;

        // Update in-memory state. Overwrites reuse the existing value
        // buffer's capacity (steady-state overwrite churn — the common
        // service pattern — then allocates nothing per put).
        if let Some(slot) = self.kv.get_mut(key) {
            slot.clear();
            slot.extend_from_slice(value);
        } else {
            self.kv.insert(String::from(key), value.to_vec());
        }
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

        let keys: Vec<String> =
            self.kv.keys().filter(|k| k.starts_with(prefix)).take(limit).cloned().collect();

        Ok(keys)
    }

    /// Sync all pending writes to durable storage.
    pub fn sync(&mut self) -> Result<(), StatefsError> {
        self.device.sync().map_err(|_| StatefsError::IoError)
    }

    /// Reopen the journal by replaying from the current device.
    /// Open transactions are discarded (crash-equivalent restart).
    pub fn reopen(&mut self) -> Result<(), StatefsError> {
        self.kv.clear();
        self.write_pos = 0;
        self.record_count = 0;
        self.replayed_count = 0;
        self.replay_orphans = 0;
        self.txns.clear();
        self.detect_layout()?;
        self.replay()
    }

    /// Consume the engine and return the underlying device (test harnesses,
    /// offline tooling).
    pub fn into_device(self) -> B {
        self.device
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

    fn write_bytes_to_device(device: &mut MemBlockDevice, bytes: &[u8]) {
        let block_size = device.block_size();
        let blocks = device.raw_storage_mut();
        let capacity = block_size * blocks.len();
        assert!(bytes.len() <= capacity, "fixture bytes exceed device capacity");
        for (idx, block) in blocks.iter_mut().enumerate() {
            block.fill(0);
            let start = idx * block_size;
            if start >= bytes.len() {
                continue;
            }
            let end = core::cmp::min(start + block_size, bytes.len());
            block[..end - start].copy_from_slice(&bytes[start..end]);
        }
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
        assert_eq!(engine.put("/state/test/big", &big_value), Err(StatefsError::ValueTooLarge));
    }

    #[test]
    fn test_reject_key_too_long() {
        let mut engine = create_engine(512, 100);
        let long_key = format!("/state/{}", "x".repeat(MAX_KEY_LEN));
        assert_eq!(engine.put(&long_key, b"value"), Err(StatefsError::KeyTooLong));
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
        assert_eq!(engine.delete("/state/nonexistent"), Err(StatefsError::NotFound));
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
            1, // version
            0, // active_slot = A
            1, // pending_slot = Some(B) encoded as 1
            3, // tries_left
            0, // health_ok = false
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
    fn test_truncated_tail_stops_replay() {
        let mut device = MemBlockDevice::new(64, 4);
        let record_a = journal_v2::encode_record(JournalOpCode::Put, "/state/test/a", b"one");
        let record_b = journal_v2::encode_record(JournalOpCode::Put, "/state/test/b", b"two");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&record_a);
        bytes.extend_from_slice(&record_b);

        // Append a truncated record header that claims more data than remains.
        bytes.extend_from_slice(&JOURNAL_MAGIC.to_le_bytes());
        bytes.push(JournalOpCode::Put as u8);
        bytes.extend_from_slice(&5u16.to_le_bytes());
        bytes.extend_from_slice(&1000u32.to_le_bytes());
        // Partial key bytes (not enough to complete record).
        bytes.extend_from_slice(b"/stat");

        write_bytes_to_device(&mut device, &bytes);
        let engine = JournalEngine::open(device).unwrap();
        assert_eq!(engine.get("/state/test/a").unwrap(), b"one");
        assert_eq!(engine.get("/state/test/b").unwrap(), b"two");
    }

    #[test]
    fn test_partial_record_boundary_replay() {
        let mut device = MemBlockDevice::new(512, 6);
        let large_value = vec![0x11u8; 1300];
        let record_large =
            journal_v2::encode_record(JournalOpCode::Put, "/state/test/large", &large_value);
        let record_tail = journal_v2::encode_record(JournalOpCode::Put, "/state/test/tail", b"ok");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&record_large);
        bytes.extend_from_slice(&record_tail);

        write_bytes_to_device(&mut device, &bytes);
        let engine = JournalEngine::open(device).unwrap();
        assert_eq!(engine.get("/state/test/large").unwrap(), large_value);
        assert_eq!(engine.get("/state/test/tail").unwrap(), b"ok");
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
