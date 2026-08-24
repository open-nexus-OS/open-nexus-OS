// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bounded sliding window over the live journal region for the
//! fsck scan (TASK-0051). The scan must run inside the in-service heap
//! (statefsd, 1 MiB bump allocator) as well as the offline CLI, so the
//! region is streamed on demand instead of materialized: O(window)
//! resident, identical walk semantics. The window invariant that keeps
//! `parse_record` honest: every `view(pos)` reaches either the region end
//! or at least `MAX_RECORD_WIRE` bytes — `parse_record` rejects
//! over-cap lengths BEFORE its truncation check, so a window-bounded view
//! can never fake a torn tail. A side effect the recovery lane relies on:
//! a clean journal only reads ~journal-length bytes off the device, not
//! the whole region (32 MiB at virtio 512 B/QD1 would eat the op budget).
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: window-spanning walk + past-window corruption below;
//! the repair/outcome semantics stay pinned by tests/fsck.rs (matrix) and
//! tools/fsck-statefs/tests/cli.rs.

use alloc::vec::Vec;

use storage::BlockDevice;

use crate::fsck::FsckFault;

/// Largest possible wire record: header + max key + max value. Anything
/// claiming more is rejected by `parse_record` before its truncation check.
pub(crate) const MAX_RECORD_WIRE: usize =
    crate::RECORD_HEADER_SIZE + crate::MAX_KEY_LEN + crate::MAX_VALUE_SIZE;

/// Sliding, block-aligned window over the live journal region.
pub(crate) struct RegionWindow<'a, D: BlockDevice> {
    device: &'a D,
    region_first: u64,
    block_size: usize,
    region_len: usize,
    /// Every view reaches region end or at least this many bytes
    /// (≥ one max-size record AND ≥ two blocks for the tail check).
    view_bytes: usize,
    /// Resident bytes cap; block-aligned so refills stay block-aligned.
    window_bytes: usize,
    buf: Vec<u8>,
    /// Region byte offset of `buf[0]` (block-aligned).
    start: usize,
    block_buf: Vec<u8>,
}

impl<'a, D: BlockDevice> RegionWindow<'a, D> {
    pub(crate) fn new(
        device: &'a D,
        region_first: u64,
        region_blocks: u64,
        block_size: usize,
    ) -> Self {
        let view_bytes = core::cmp::max(MAX_RECORD_WIRE, 2 * block_size);
        let window_bytes = (2 * view_bytes).div_ceil(block_size) * block_size;
        let region_len = (region_blocks as usize).saturating_mul(block_size);
        Self {
            device,
            region_first,
            block_size,
            region_len,
            view_bytes,
            window_bytes,
            buf: Vec::with_capacity(core::cmp::min(window_bytes, region_len)),
            start: 0,
            block_buf: alloc::vec![0u8; block_size],
        }
    }

    pub(crate) fn region_len(&self) -> usize {
        self.region_len
    }

    /// Loaded bytes at `pos`, guaranteed to reach the region end or at
    /// least `view_bytes` — callers may treat a short view as "the region
    /// ends here", never as a window artifact.
    pub(crate) fn view(&mut self, pos: usize) -> Result<&[u8], FsckFault> {
        let pos = core::cmp::min(pos, self.region_len);
        self.ensure(pos)?;
        Ok(&self.buf[pos - self.start..])
    }

    fn ensure(&mut self, pos: usize) -> Result<(), FsckFault> {
        let need_end = core::cmp::min(pos.saturating_add(self.view_bytes), self.region_len);
        if pos >= self.start && need_end <= self.start + self.buf.len() {
            return Ok(());
        }
        // Re-anchor at the block containing `pos`, keeping the overlap so
        // a monotone scan reads every region block at most once.
        let new_start = (pos / self.block_size) * self.block_size;
        let new_end = core::cmp::min(new_start.saturating_add(self.window_bytes), self.region_len);
        if new_start >= self.start && new_start <= self.start + self.buf.len() {
            let keep_from = new_start - self.start;
            self.buf.copy_within(keep_from.., 0);
            let kept = self.buf.len() - keep_from;
            self.buf.truncate(kept);
        } else {
            self.buf.clear();
        }
        self.start = new_start;
        let mut off = new_start + self.buf.len();
        while off < new_end {
            let block = self.region_first + (off / self.block_size) as u64;
            self.device.read_block(block, &mut self.block_buf).map_err(|_| FsckFault {
                offset: block.saturating_mul(self.block_size as u64),
                reason: "device read failed",
            })?;
            let take = core::cmp::min(self.block_size, new_end - off);
            self.buf.extend_from_slice(&self.block_buf[..take]);
            off += take;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::vec::Vec;

    use storage::MemBlockDevice;

    use crate::fsck::{fsck, FsckOutcome};
    use crate::journal_v2::{encode_record, JournalOpCode};

    /// Raw legacy-v1 image: records written back-to-back from block 0.
    fn raw_v1_image(blocks: u64, records: &[Vec<u8>]) -> MemBlockDevice {
        let mut device = MemBlockDevice::new(512, blocks);
        let storage = device.raw_storage_mut();
        let mut pos = 0usize;
        for record in records {
            for &b in record.iter() {
                storage[pos / 512][pos % 512] = b;
                pos += 1;
            }
        }
        device
    }

    fn put_records(count: usize, fill: u8) -> Vec<Vec<u8>> {
        let value = [fill; 4096];
        (0..count)
            .map(|i| encode_record(JournalOpCode::Put, &format!("/state/win/k{i:03}"), &value))
            .collect()
    }

    /// The walk must slide across multiple windows without changing the
    /// verdict — journal longer than 2x the resident window.
    #[test]
    fn streaming_scan_walks_past_the_window() {
        let records = put_records(80, 0xA5);
        let total: usize = records.iter().map(|r| r.len()).sum();
        assert!(total > 2 * 2 * super::MAX_RECORD_WIRE, "fixture must span >2 windows");
        let (report, device) = fsck(raw_v1_image(1024, &records), false);
        assert_eq!(report.outcome, FsckOutcome::Clean);
        assert_eq!(report.records, 80);
        assert_eq!(report.entries, 80);
        assert!(device.is_some());
    }

    /// Mid-journal corruption DEEP past the first window: the streaming
    /// lookahead must still find the valid records after it (fatal), with
    /// the fault offset at the corrupted record.
    #[test]
    fn test_reject_mid_journal_corruption_beyond_first_window() {
        let records = put_records(60, 0x5A);
        let corrupt_at: usize = records[..50].iter().map(|r| r.len()).sum();
        assert!(corrupt_at > 2 * super::MAX_RECORD_WIRE, "corruption must sit past window 1");
        let mut device = raw_v1_image(1024, &records);
        // Flip a byte inside record 50: its CRC breaks, records 51.. stay valid.
        let p = corrupt_at + 20;
        device.raw_storage_mut()[p / 512][p % 512] ^= 0xFF;
        let (report, device) = fsck(device, false);
        assert_eq!(report.outcome, FsckOutcome::Unrecoverable);
        let f = report.fault.expect("fault");
        assert_eq!(f.offset, corrupt_at as u64);
        assert_eq!(f.reason, "crc mismatch");
        assert!(device.is_none());
    }
}
