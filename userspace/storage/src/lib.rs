// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Block device abstractions for userspace storage backends
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests in downstream crates
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

/// Block device error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// Read/write failed.
    IoError,
    /// Block index out of range.
    OutOfRange,
}

/// Abstract block device for storage backend.
pub trait BlockDevice {
    /// Block size in bytes (typically 512).
    fn block_size(&self) -> usize;

    /// Total number of blocks.
    fn block_count(&self) -> u64;

    /// Read a single block into buffer.
    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError>;

    /// Write a single block from buffer.
    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError>;

    /// Flush all pending writes to durable storage.
    fn sync(&mut self) -> Result<(), BlockError>;
}

/// In-memory block device for testing.
pub struct MemBlockDevice {
    block_size: usize,
    blocks: Vec<Vec<u8>>,
}

impl MemBlockDevice {
    /// Create a new memory block device with given block size and count.
    pub fn new(block_size: usize, block_count: u64) -> Self {
        let blocks = (0..block_count)
            .map(|_| vec![0u8; block_size])
            .collect();
        Self { block_size, blocks }
    }

    /// Get raw access to storage (for corruption tests and fixtures).
    pub fn raw_storage_mut(&mut self) -> &mut [Vec<u8>] {
        &mut self.blocks
    }
}

impl BlockDevice for MemBlockDevice {
    fn block_size(&self) -> usize {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.blocks.len() as u64
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        let idx = block_idx as usize;
        if idx >= self.blocks.len() {
            return Err(BlockError::OutOfRange);
        }
        if buf.len() < self.block_size {
            return Err(BlockError::IoError);
        }
        buf[..self.block_size].copy_from_slice(&self.blocks[idx]);
        Ok(())
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        let idx = block_idx as usize;
        if idx >= self.blocks.len() {
            return Err(BlockError::OutOfRange);
        }
        if buf.len() < self.block_size {
            return Err(BlockError::IoError);
        }
        self.blocks[idx].copy_from_slice(&buf[..self.block_size]);
        Ok(())
    }

    fn sync(&mut self) -> Result<(), BlockError> {
        // In-memory: no-op.
        Ok(())
    }
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub mod virtio_blk;
