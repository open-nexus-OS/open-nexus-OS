// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Logical-block adapter over `storage::BlockDevice` — nxfs speaks
//! 4 KiB logical blocks; the underlying device may use smaller sectors
//! (virtio-blk: 512 B). Bounded, byte-exact, no partial-sector writes.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0292)
//! TEST_COVERAGE: adapter roundtrip test below

use alloc::vec;

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
        let sector_len = self.inner.block_size();
        let mut sector = vec![0u8; sector_len];
        for i in 0..self.sectors_per_block {
            self.inner
                .read_block(lb * self.sectors_per_block + i, &mut sector)
                .map_err(|_| NxfsError::Io)?;
            let off = (i as usize) * sector_len;
            out[off..off + sector_len].copy_from_slice(&sector);
        }
        Ok(())
    }

    pub(crate) fn write(&mut self, lb: u64, data: &[u8; LOGICAL_BLOCK_SIZE]) -> Result<()> {
        if lb >= self.logical_blocks {
            return Err(NxfsError::Io);
        }
        let sector_len = self.inner.block_size();
        for i in 0..self.sectors_per_block {
            let off = (i as usize) * sector_len;
            self.inner
                .write_block(lb * self.sectors_per_block + i, &data[off..off + sector_len])
                .map_err(|_| NxfsError::Io)?;
        }
        Ok(())
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

    /// Reads `len` bytes starting at `lb`.
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
}
