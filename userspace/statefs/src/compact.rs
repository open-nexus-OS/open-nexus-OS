// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS bounded compaction (TASK-0026, journal v2 generations).
//! Mirrors the nxfs checkpoint-flip discipline: the snapshot is written into
//! the INACTIVE half of the device, then a single-block `NXS2` superblock
//! write flips the active region atomically — a crash at any point leaves
//! either the old journal or the new snapshot fully intact, never a hybrid.
//! Replay after compaction scans only the live region (incremental replay:
//! reopen cost is proportional to the live journal, not lifetime writes).
//! OWNERS: @runtime
//! STATUS: Functional (host-first)
//! API_STABILITY: Unstable (v2a)
//! TEST_COVERAGE: Unit tests below + tests/crash_injection.rs (threshold,
//! v1→v2 upgrade, incremental-replay op-count, crash cuts)

use alloc::vec;

use storage::BlockDevice;

use crate::journal_v2::{encode_checkpoint, encode_record, JournalOpCode};
use crate::{JournalEngine, StatefsError, RECORD_HEADER_SIZE};

// ============================================================================
// v2 superblock (block 0, present from generation 1 on)
// ============================================================================

/// Superblock magic: b"NXS2" (distinct from the record magic "NXSF", whose
/// on-disk byte order is `46 53 58 4E`).
pub(crate) const SUPERBLOCK_MAGIC: [u8; 4] = *b"NXS2";
const SUPERBLOCK_VERSION: u8 = 1;
/// magic(4) + version(1) + active(1) + reserved(2) + gen(4) + entries(4) + crc(4)
const SUPERBLOCK_LEN: usize = 20;

/// Compaction thresholds and work bounds. All bounds are hard caps.
#[derive(Debug, Clone, Copy)]
pub struct CompactionConfig {
    /// Never compact below this journal size (bytes).
    pub min_journal_bytes: usize,
    /// Compact when `journal_bytes >= live_bytes * ratio` (garbage ratio).
    pub ratio: u32,
    /// Hard cap on snapshot entries written in one cycle; a store larger
    /// than this defers compaction (loudly, via `compact_now` error).
    pub max_entries_per_cycle: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self { min_journal_bytes: 8192, ratio: 2, max_entries_per_cycle: 8192 }
    }
}

/// Observable per-cycle compaction work (bounded by live state + config,
/// never by lifetime writes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionStats {
    /// Generation of the journal written by this cycle.
    pub generation: u32,
    /// Live entries snapshotted.
    pub entries: usize,
    /// Snapshot bytes written (checkpoint record + Put records).
    pub bytes_written: usize,
    /// Device blocks written this cycle (snapshot + tail terminator +
    /// superblock).
    pub blocks_written: usize,
}

fn encode_superblock(active: u8, generation: u32, entries: u32) -> [u8; SUPERBLOCK_LEN] {
    let mut buf = [0u8; SUPERBLOCK_LEN];
    buf[0..4].copy_from_slice(&SUPERBLOCK_MAGIC);
    buf[4] = SUPERBLOCK_VERSION;
    buf[5] = active;
    // buf[6..8] reserved = 0
    buf[8..12].copy_from_slice(&generation.to_le_bytes());
    buf[12..16].copy_from_slice(&entries.to_le_bytes());
    let crc = crate::crc32c(&buf[..16]);
    buf[16..20].copy_from_slice(&crc.to_le_bytes());
    buf
}

/// Parse a superblock from block-0 bytes. `None` = not a v2 superblock
/// (legacy v1 journal layout).
pub(crate) fn parse_superblock(block: &[u8]) -> Option<(u8, u32, u32)> {
    if block.len() < SUPERBLOCK_LEN || block[0..4] != SUPERBLOCK_MAGIC {
        return None;
    }
    if block[4] != SUPERBLOCK_VERSION {
        return None;
    }
    let active = block[5];
    if active > 1 {
        return None;
    }
    let stored_crc = u32::from_le_bytes([block[16], block[17], block[18], block[19]]);
    if crate::crc32c(&block[..16]) != stored_crc {
        return None;
    }
    let generation = u32::from_le_bytes([block[8], block[9], block[10], block[11]]);
    let entries = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
    Some((active, generation, entries))
}

/// Region geometry for a v2-layout device: block 0 = superblock, then two
/// equal regions A and B. Returns `(region_blocks, a_first, b_first)`;
/// `None` = device too small for the A/B layout.
pub(crate) fn region_geometry(block_count: u64) -> Option<(u64, u64, u64)> {
    let region_blocks = block_count.checked_sub(1)? / 2;
    if region_blocks == 0 {
        return None;
    }
    Some((region_blocks, 1, 1 + region_blocks))
}

// ============================================================================
// Engine integration
// ============================================================================

impl<B: BlockDevice> JournalEngine<B> {
    /// Detect the journal layout: a valid `NXS2` superblock in block 0
    /// selects the v2 A/B region layout; anything else is the legacy v1
    /// layout (records from block 0, whole device).
    pub(crate) fn detect_layout(&mut self) -> Result<(), StatefsError> {
        let block_size = self.device.block_size();
        let block_count = self.device.block_count();
        // Legacy defaults.
        self.region_first_block = 0;
        self.region_block_count = block_count;
        self.generation = 0;
        if block_count == 0 || block_size < SUPERBLOCK_LEN {
            return Ok(());
        }
        let mut block = vec![0u8; block_size];
        self.device.read_block(0, &mut block).map_err(|_| StatefsError::IoError)?;
        if let Some((active, generation, _entries)) = parse_superblock(&block) {
            let (region_blocks, a_first, b_first) =
                region_geometry(block_count).ok_or(StatefsError::Corrupted)?;
            self.region_first_block = if active == 0 { a_first } else { b_first };
            self.region_block_count = region_blocks;
            self.generation = generation;
        }
        Ok(())
    }

    /// Serialized size of the live state as a fresh snapshot journal.
    fn live_bytes(&self) -> usize {
        let mut total = RECORD_HEADER_SIZE + 8; // checkpoint record
        for (key, value) in &self.kv {
            total += RECORD_HEADER_SIZE + key.len() + value.len();
        }
        total
    }

    /// Compact if the configured threshold is reached and a cycle is
    /// currently possible (no open transactions, entry cap, geometry).
    /// `Ok(None)` = nothing to do or deferred; never an error for deferral.
    pub fn maybe_compact(&mut self) -> Result<Option<CompactionStats>, StatefsError> {
        let cfg = self.compaction;
        if self.write_pos < cfg.min_journal_bytes {
            return Ok(None);
        }
        let live = self.live_bytes();
        if (self.write_pos as u64) < (live as u64).saturating_mul(cfg.ratio as u64) {
            return Ok(None);
        }
        if !self.txns.is_empty() || self.kv.len() > cfg.max_entries_per_cycle {
            return Ok(None); // deferred, retried on a later cycle
        }
        if self.compaction_target().is_err() {
            return Ok(None); // geometry does not allow a crash-safe cycle
        }
        self.compact_now().map(Some)
    }

    /// Pick the inactive target region for the next snapshot.
    /// Errors (`IoError`): device too small, or a legacy v1 journal already
    /// extends into region B (no crash-safe first compaction possible).
    fn compaction_target(&self) -> Result<(u64, u64, u8), StatefsError> {
        let block_size = self.device.block_size();
        if block_size < SUPERBLOCK_LEN {
            return Err(StatefsError::IoError);
        }
        let (region_blocks, a_first, b_first) =
            region_geometry(self.device.block_count()).ok_or(StatefsError::IoError)?;
        if self.region_first_block == 0 {
            // Legacy layout: first compaction targets region B, which must
            // not overlap the live v1 journal (block 0..used stays intact
            // until the superblock flip).
            let used_blocks = self.write_pos.div_ceil(block_size) as u64;
            if used_blocks > b_first {
                return Err(StatefsError::IoError);
            }
            Ok((b_first, region_blocks, 1))
        } else if self.region_first_block == a_first {
            Ok((b_first, region_blocks, 1))
        } else {
            Ok((a_first, region_blocks, 0))
        }
    }

    /// Run one bounded compaction cycle: write `CHECKPOINT{gen+1, n}` plus
    /// one v1 `Put` record per live key into the inactive region, sync,
    /// then flip the superblock (single-block write), sync again.
    ///
    /// Work this cycle is O(live entries + snapshot blocks), hard-capped by
    /// `max_entries_per_cycle` and the region size — never proportional to
    /// lifetime writes. Errors (`IoError`): open transactions (commit or
    /// abort them first), entry cap exceeded, snapshot does not fit, or
    /// device I/O failed.
    pub fn compact_now(&mut self) -> Result<CompactionStats, StatefsError> {
        if !self.txns.is_empty() {
            // Open txns live in the CURRENT region's byte order; rotating
            // now would orphan their PREPARE/PAYLOAD records.
            return Err(StatefsError::IoError);
        }
        let entries = self.kv.len();
        if entries > self.compaction.max_entries_per_cycle {
            return Err(StatefsError::IoError);
        }
        let (target_first, region_blocks, target_active) = self.compaction_target()?;
        let block_size = self.device.block_size();
        let new_gen = self.generation.saturating_add(1);

        // Serialize the snapshot: checkpoint boundary, then live Puts in
        // deterministic (BTreeMap) key order — plain v1 records.
        let mut snapshot = encode_checkpoint(new_gen, entries as u32);
        for (key, value) in &self.kv {
            snapshot.extend_from_slice(&encode_record(JournalOpCode::Put, key, value));
        }
        let snap_blocks = snapshot.len().div_ceil(block_size) as u64;
        // +1: the block after the snapshot is zeroed (tail terminator) so
        // stale records from an older generation can never be replayed.
        if snap_blocks.saturating_add(1) > region_blocks {
            return Err(StatefsError::IoError);
        }

        // Phase 1: snapshot into the inactive region (old journal intact).
        let mut buf = vec![0u8; block_size];
        for i in 0..snap_blocks as usize {
            let start = i * block_size;
            let end = core::cmp::min(start + block_size, snapshot.len());
            buf.fill(0);
            buf[..end - start].copy_from_slice(&snapshot[start..end]);
            self.device
                .write_block(target_first + i as u64, &buf)
                .map_err(|_| StatefsError::IoError)?;
        }
        buf.fill(0);
        self.device
            .write_block(target_first + snap_blocks, &buf)
            .map_err(|_| StatefsError::IoError)?;
        self.device.sync().map_err(|_| StatefsError::IoError)?;

        // Phase 2: atomic flip — one superblock write selects the new
        // generation; a crash before this point replays the old journal.
        let superblock = encode_superblock(target_active, new_gen, entries as u32);
        buf.fill(0);
        buf[..SUPERBLOCK_LEN].copy_from_slice(&superblock);
        self.device.write_block(0, &buf).map_err(|_| StatefsError::IoError)?;
        self.device.sync().map_err(|_| StatefsError::IoError)?;

        // Adopt the new region as the live journal.
        self.region_first_block = target_first;
        self.region_block_count = region_blocks;
        self.generation = new_gen;
        self.write_pos = snapshot.len();
        self.record_count = entries + 1;
        Ok(CompactionStats {
            generation: new_gen,
            entries,
            bytes_written: snapshot.len(),
            blocks_written: snap_blocks as usize + 2,
        })
    }

    /// Replace the compaction thresholds/bounds.
    pub fn set_compaction_config(&mut self, config: CompactionConfig) {
        self.compaction = config;
    }

    /// Journal generation (0 = never compacted, legacy v1 layout).
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Current live-journal size in bytes (region-relative write head).
    pub fn journal_bytes(&self) -> usize {
        self.write_pos
    }

    /// Records scanned by the most recent replay (open/reopen). The
    /// incremental-replay contract: bounded by live state + appends since
    /// the last compaction, independent of lifetime writes.
    pub fn replayed_records(&self) -> usize {
        self.replayed_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superblock_roundtrip_and_rejects() {
        let sb = encode_superblock(1, 7, 42);
        assert_eq!(parse_superblock(&sb), Some((1, 7, 42)));
        // CRC damage.
        let mut bad = sb;
        bad[8] ^= 0xFF;
        assert_eq!(parse_superblock(&bad), None);
        // Invalid active region.
        let mut sb2 = encode_superblock(2, 1, 0);
        let crc = crate::crc32c(&sb2[..16]);
        sb2[16..20].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(parse_superblock(&sb2), None);
        // Record magic is not a superblock.
        assert_eq!(parse_superblock(&crate::JOURNAL_MAGIC.to_le_bytes()), None);
    }

    #[test]
    fn region_geometry_bounds() {
        assert_eq!(region_geometry(0), None);
        assert_eq!(region_geometry(1), None);
        assert_eq!(region_geometry(2), None);
        assert_eq!(region_geometry(3), Some((1, 1, 2)));
        assert_eq!(region_geometry(64), Some((31, 1, 32)));
    }
}
