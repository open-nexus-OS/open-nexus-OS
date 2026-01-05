// Copyright 2025 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host-side unit tests for the VirtIO block driver stub (capacity + read path).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test
//!
//! TEST_SCOPE:
//!   - Capacity report behavior
//!   - Read-block call wiring against a stub Bus/DMA buffer
//!
//! TEST_SCENARIOS:
//!   - capacity_and_read(): Capacity is computed and read path is callable
//!
//! DEPENDENCIES:
//!   - nexus_hal::{Bus, DmaBuffer}: test stubs
//!   - storage_virtio_blk::VirtioBlk: driver under test
//!
//! ADR: docs/architecture/01-neuron-kernel.md

use nexus_hal::{Bus, DmaBuffer};
use storage_virtio_blk::VirtioBlk;

struct BusStub;

impl Bus for BusStub {
    fn read(&self, addr: usize) -> u32 {
        if addr == 0 {
            0
        } else {
            1
        }
    }

    fn write(&self, _addr: usize, _value: u32) {}
}

struct BufStub;

impl DmaBuffer for BufStub {
    fn as_ptr(&self) -> *const u8 {
        core::ptr::null()
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        core::ptr::null_mut()
    }

    fn len(&self) -> usize {
        512
    }
}

#[test]
fn capacity_and_read() {
    let blk = VirtioBlk::new(BusStub);
    assert_eq!(blk.capacity(), 1_u64 << 32);
    let mut buf = BufStub;
    blk.read_block(&mut buf);
    assert_eq!(buf.len(), 512);
}
