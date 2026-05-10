// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Safe wrapper for virtio-blk MMIO backend
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (OS-only bring-up)
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use storage_virtio_blk::VirtioBlkMmio;

use crate::{BlockDevice, BlockError};

/// BlockDevice wrapper over virtio-blk MMIO backend.
pub struct VirtioBlkDevice {
    inner: VirtioBlkMmio,
}

impl VirtioBlkDevice {
    pub fn new(mmio_cap_slot: u32) -> Result<Self, BlockError> {
        let inner = VirtioBlkMmio::new(mmio_cap_slot).map_err(|_| BlockError::IoError)?;
        Ok(Self { inner })
    }

    pub fn capacity_sectors(&self) -> u64 {
        self.inner.capacity_sectors()
    }

    pub fn sector_size(&self) -> u32 {
        self.inner.sector_size()
    }
}

impl BlockDevice for VirtioBlkDevice {
    fn block_size(&self) -> usize {
        self.inner.sector_size() as usize
    }

    fn block_count(&self) -> u64 {
        self.inner.capacity_sectors()
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.inner.read_block(block_idx, buf).map_err(|_| BlockError::IoError)
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        self.inner.write_block(block_idx, buf).map_err(|_| BlockError::IoError)
    }

    fn sync(&mut self) -> Result<(), BlockError> {
        self.inner.sync().map_err(|_| BlockError::IoError)
    }
}
