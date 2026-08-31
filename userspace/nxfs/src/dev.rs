// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Logical-block adapter over `storage::BlockDevice` — nxfs speaks
//! 4 KiB logical blocks; the underlying device may use smaller sectors
//! (virtio-blk: 512 B). Bounded, byte-exact, no partial-sector writes.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0292)
//! TEST_COVERAGE: adapter roundtrip test below

use storage::BlockDevice;

use crate::format::LOGICAL_BLOCK_SIZE;
use crate::{NxfsError, Result};

/// Logical-block device view.
pub(crate) struct Dev<D: BlockDevice> {
    inner: D,
    sectors_per_block: u64,
    logical_blocks: u64,
}

impl<D: BlockDevice> Dev<D> {
    pub(crate) fn new(inner: D) -> Result<Self> {
        let sector = inner.block_size();
        // `%` not `is_multiple_of`: the OS toolchain (nightly-2025-01-15) predates
        // the `unsigned_is_multiple_of` stabilization (stable 1.87).
        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        let bad_sector = sector == 0 || LOGICAL_BLOCK_SIZE % sector != 0;
        if bad_sector {
            return Err(NxfsError::Io);
        }
        let sectors_per_block = (LOGICAL_BLOCK_SIZE / sector) as u64;
        let logical_blocks = inner.block_count() / sectors_per_block;
        Ok(Self { inner, sectors_per_block, logical_blocks })
    }

    pub(crate) fn logical_blocks(&self) -> u64 {
        self.logical_blocks
    }

    pub(crate) fn read(&self, lb: u64, out: &mut [u8; LOGICAL_BLOCK_SIZE]) -> Result<()> {
        if lb >= self.logical_blocks {
            return Err(NxfsError::Io);
        }
        // TASK-0314: one RUN request per logical block (was: 8 serialized
        // sector requests + a per-read heap `vec!` bounce buffer).
        self.inner.read_blocks(lb * self.sectors_per_block, out).map_err(|_| NxfsError::Io)
    }

    pub(crate) fn write(&mut self, lb: u64, data: &[u8; LOGICAL_BLOCK_SIZE]) -> Result<()> {
        if lb >= self.logical_blocks {
            return Err(NxfsError::Io);
        }
        // TASK-0314: one RUN request per logical block (was: 8 serialized
        // sector writes).
        self.inner.write_blocks(lb * self.sectors_per_block, data).map_err(|_| NxfsError::Io)
    }

    /// Writes an arbitrary byte run starting at `lb` (zero-padded tail).
    pub(crate) fn write_bytes(&mut self, lb: u64, bytes: &[u8]) -> Result<()> {
        let mut block = [0u8; LOGICAL_BLOCK_SIZE];
        for (idx, chunk) in bytes.chunks(LOGICAL_BLOCK_SIZE).enumerate() {
            block[..chunk.len()].copy_from_slice(chunk);
            block[chunk.len()..].fill(0);
            self.write(lb + idx as u64, &block)?;
        }
        Ok(())
    }

    /// Fills `buf` from block `lb` onward — ALLOCATION-FREE (the caller
    /// owns the buffer). The streaming read/CoW paths use this; a
    /// per-window `Vec` would be fatal on the never-freeing service bump
    /// heaps (a 19 MB container = 290 windows = 19 MB of leaked arena).
    pub(crate) fn read_into(&self, lb: u64, buf: &mut [u8]) -> Result<()> {
        let mut block = [0u8; LOGICAL_BLOCK_SIZE];
        let mut done = 0usize;
        let mut idx = 0u64;
        while done < buf.len() {
            self.read(lb + idx, &mut block)?;
            let take = (buf.len() - done).min(LOGICAL_BLOCK_SIZE);
            buf[done..done + take].copy_from_slice(&block[..take]);
            done += take;
            idx += 1;
        }
        Ok(())
    }

    /// Reads `len` bytes starting at `lb`. Allocating — reserved for the
    /// BOUNDED mount-time paths (checkpoint blob, journal replay); every
    /// per-request path uses `read_into`.
    pub(crate) fn read_bytes(&self, lb: u64, len: usize) -> Result<alloc::vec::Vec<u8>> {
        let mut out = alloc::vec::Vec::with_capacity(len);
        let mut block = [0u8; LOGICAL_BLOCK_SIZE];
        let blocks = len.div_ceil(LOGICAL_BLOCK_SIZE) as u64;
        for i in 0..blocks {
            self.read(lb + i, &mut block)?;
            let remaining = len - out.len();
            out.extend_from_slice(&block[..remaining.min(LOGICAL_BLOCK_SIZE)]);
        }
        Ok(out)
    }

    pub(crate) fn sync(&mut self) -> Result<()> {
        self.inner.sync().map_err(|_| NxfsError::Io)
    }

    pub(crate) fn into_inner(self) -> D {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::MemBlockDevice;

    #[test]
    fn adapter_roundtrips_across_sector_sizes() {
        for sector in [512usize, 4096] {
            let sectors = (64 * LOGICAL_BLOCK_SIZE / sector) as u64;
            let mut dev = Dev::new(MemBlockDevice::new(sector, sectors)).expect("dev");
            assert_eq!(dev.logical_blocks(), 64);
            let mut block = [0u8; LOGICAL_BLOCK_SIZE];
            block[0] = 0xAA;
            block[LOGICAL_BLOCK_SIZE - 1] = 0x55;
            dev.write(3, &block).expect("write");
            let mut back = [0u8; LOGICAL_BLOCK_SIZE];
            dev.read(3, &mut back).expect("read");
            assert_eq!(back, block, "sector={sector}");
            // Byte-run API
            let run: alloc::vec::Vec<u8> = (0..9000u32).map(|i| i as u8).collect();
            dev.write_bytes(10, &run).expect("write bytes");
            assert_eq!(dev.read_bytes(10, run.len()).expect("read bytes"), run);
        }
    }

    /// TASK-0314: a run-capable device sees exactly ONE run request per
    /// logical block (v1 issued 8 serialized sector requests), and the run
    /// path is byte-identical to the per-sector default.
    #[test]
    fn logical_block_uses_one_run_request() {
        use core::cell::Cell;
        use storage::{BlockDevice, BlockError};

        struct CountingDev {
            inner: MemBlockDevice,
            sector_calls: Cell<u32>,
            run_calls: Cell<u32>,
        }
        impl BlockDevice for CountingDev {
            fn block_size(&self) -> usize {
                self.inner.block_size()
            }
            fn block_count(&self) -> u64 {
                self.inner.block_count()
            }
            fn read_block(&self, i: u64, b: &mut [u8]) -> core::result::Result<(), BlockError> {
                self.sector_calls.set(self.sector_calls.get() + 1);
                self.inner.read_block(i, b)
            }
            fn write_block(&mut self, i: u64, b: &[u8]) -> core::result::Result<(), BlockError> {
                self.sector_calls.set(self.sector_calls.get() + 1);
                self.inner.write_block(i, b)
            }
            fn read_blocks(
                &self,
                first: u64,
                buf: &mut [u8],
            ) -> core::result::Result<(), BlockError> {
                self.run_calls.set(self.run_calls.get() + 1);
                // Delegate to the inner DEFAULT (sector loop) for data truth
                // without touching our sector counter.
                self.inner.read_blocks(first, buf)
            }
            fn write_blocks(
                &mut self,
                first: u64,
                buf: &[u8],
            ) -> core::result::Result<(), BlockError> {
                self.run_calls.set(self.run_calls.get() + 1);
                self.inner.write_blocks(first, buf)
            }
            fn sync(&mut self) -> core::result::Result<(), BlockError> {
                self.inner.sync()
            }
        }

        let mut plain = Dev::new(MemBlockDevice::new(512, 128 * 8)).expect("plain");
        let mut counting = Dev::new(CountingDev {
            inner: MemBlockDevice::new(512, 128 * 8),
            sector_calls: Cell::new(0),
            run_calls: Cell::new(0),
        })
        .expect("counting");

        let mut block = [0u8; LOGICAL_BLOCK_SIZE];
        for (i, byte) in block.iter_mut().enumerate() {
            *byte = (i % 251) as u8;
        }
        plain.write(5, &block).expect("plain write");
        counting.write(5, &block).expect("counting write");
        assert_eq!(counting.inner.run_calls.get(), 1, "one run per logical-block write");
        assert_eq!(counting.inner.sector_calls.get(), 0, "no per-sector calls on the hot path");

        let mut a = [0u8; LOGICAL_BLOCK_SIZE];
        let mut b = [0u8; LOGICAL_BLOCK_SIZE];
        plain.read(5, &mut a).expect("plain read");
        counting.read(5, &mut b).expect("counting read");
        assert_eq!(a, b, "run path is byte-identical to the sector loop");
        assert_eq!(counting.inner.run_calls.get(), 2, "one run per logical-block read");
        assert_eq!(counting.inner.sector_calls.get(), 0);
    }
}
