// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The trace tests' disks (test-only): a memory disk that holds only the `trace`
//! partition, and a disk whose writes fail where a test says — a power cut at that write.
//! OWNERS: @runtime @devx
//! STATUS: Functional
//! API_STABILITY: Internal (tests)
//! TEST_COVERAGE: used by the loader's and the OS writer's tests

use super::*;
use crate::gpt::write_gpt;
use crate::MemBlockDevice;

/// A disk with only the trace partition, at LBA 2048.
pub(crate) fn disk() -> MemBlockDevice {
    let sectors = u64::from(SLOTS) * SLOT_SECTORS;
    let mut dev = MemBlockDevice::new(SECTOR, 2048 + sectors + 64);
    let part = Partition {
        type_guid: GUID_NEXUS_TRACE,
        first_lba: 2048,
        last_lba: 2048 + sectors - 1,
        name: PARTITION.into(),
    };
    write_gpt(&mut dev, &[part]).expect("gpt");
    dev
}

pub(crate) fn part_of(dev: &MemBlockDevice) -> Partition {
    partition(&parse_gpt(dev).expect("gpt")).expect("trace")
}

/// A disk whose writes fail where `fails(lba, bytes)` says: a power cut at that write.
pub(crate) struct Torn<F: Fn(u64, &[u8]) -> bool> {
    pub(crate) dev: MemBlockDevice,
    pub(crate) fails: F,
}

impl<F: Fn(u64, &[u8]) -> bool> BlockDevice for Torn<F> {
    fn block_size(&self) -> usize {
        self.dev.block_size()
    }
    fn block_count(&self) -> u64 {
        self.dev.block_count()
    }
    fn read_block(&self, idx: u64, buf: &mut [u8]) -> Result<(), crate::BlockError> {
        self.dev.read_block(idx, buf)
    }
    fn write_block(&mut self, idx: u64, buf: &[u8]) -> Result<(), crate::BlockError> {
        if (self.fails)(idx, buf) {
            return Err(crate::BlockError::IoError);
        }
        self.dev.write_block(idx, buf)
    }
    fn sync(&mut self) -> Result<(), crate::BlockError> {
        self.dev.sync()
    }
}
