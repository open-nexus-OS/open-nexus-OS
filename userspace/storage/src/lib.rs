// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Block device abstractions for userspace storage backends
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: In-crate unit tests for `pkgimg` contract/reject paths + downstream integration tests
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

    /// Read a contiguous RUN of blocks (`buf.len()` = n × block_size) —
    /// default: sector loop; devices with a run-capable transport override
    /// this with ONE request per run (TASK-0314).
    fn read_blocks(&self, first_block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        let bs = self.block_size();
        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        let misaligned = bs == 0 || buf.len() % bs != 0;
        if misaligned {
            return Err(BlockError::OutOfRange);
        }
        for (i, chunk) in buf.chunks_mut(bs).enumerate() {
            self.read_block(first_block + i as u64, chunk)?;
        }
        Ok(())
    }

    /// Write a contiguous RUN of blocks (`buf.len()` = n × block_size) —
    /// default: sector loop; see `read_blocks`.
    fn write_blocks(&mut self, first_block: u64, buf: &[u8]) -> Result<(), BlockError> {
        let bs = self.block_size();
        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        let misaligned = bs == 0 || buf.len() % bs != 0;
        if misaligned {
            return Err(BlockError::OutOfRange);
        }
        for (i, chunk) in buf.chunks(bs).enumerate() {
            self.write_block(first_block + i as u64, chunk)?;
        }
        Ok(())
    }

    /// Flush all pending writes to durable storage.
    fn sync(&mut self) -> Result<(), BlockError>;
}

/// In-memory block device for testing.
/// In-memory block device. SPARSE by construction: only blocks that were
/// actually written occupy memory, and unwritten blocks read as zero —
/// exactly like a freshly provisioned device. Dense preallocation made a
/// full-layout fixture (384 MiB) cost its whole size per test instance,
/// and eight parallel tests were OOM-killed by it.
pub struct MemBlockDevice {
    block_size: usize,
    block_count: u64,
    blocks: alloc::collections::BTreeMap<u64, Vec<u8>>,
}

impl MemBlockDevice {
    /// Create a new memory block device with given block size and count.
    pub fn new(block_size: usize, block_count: u64) -> Self {
        Self { block_size, block_count, blocks: alloc::collections::BTreeMap::new() }
    }

    /// Mutable access to ONE block, materializing it on demand (corruption
    /// tests and fixtures). `None` when the index is out of range.
    pub fn raw_block_mut(&mut self, block_idx: u64) -> Option<&mut Vec<u8>> {
        if block_idx >= self.block_count {
            return None;
        }
        let size = self.block_size;
        Some(self.blocks.entry(block_idx).or_insert_with(|| vec![0u8; size]))
    }
}

impl BlockDevice for MemBlockDevice {
    fn block_size(&self) -> usize {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        if block_idx >= self.block_count {
            return Err(BlockError::OutOfRange);
        }
        if buf.len() < self.block_size {
            return Err(BlockError::IoError);
        }
        match self.blocks.get(&block_idx) {
            Some(block) => buf[..self.block_size].copy_from_slice(block),
            // Never written: reads as zero, like fresh media.
            None => buf[..self.block_size].fill(0),
        }
        Ok(())
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        if block_idx >= self.block_count {
            return Err(BlockError::OutOfRange);
        }
        if buf.len() < self.block_size {
            return Err(BlockError::IoError);
        }
        let size = self.block_size;
        self.blocks
            .entry(block_idx)
            .or_insert_with(|| vec![0u8; size])
            .copy_from_slice(&buf[..size]);
        Ok(())
    }

    fn sync(&mut self) -> Result<(), BlockError> {
        // In-memory: no-op.
        Ok(())
    }
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub mod virtio_blk;

/// Partition-scoped block client over blockproto (ADR-0044/TASK-0315).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub mod remote_blk;

/// Deterministic PackageFS image v2 (`pkgimg`) format helpers.
pub mod pkgimg;

/// `pkgimg` v3 — system-volume image with bundle table + launch params
/// (RFC-0089 §12.1, TASK-0321).
pub mod pkgimg_bundles;

/// Bounded read-only GPT parsing + `PartitionView` (ADR-0044).
pub mod gpt;

/// THE RFC-0089 §2 disk-layout authority (shared by nx image, virtioblkd,
/// nxboot — one table, zero drift).
pub mod layout;

/// Partition-scoped block IPC protocol codec (ADR-0044).
pub mod blockproto;
