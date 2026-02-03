// Copyright 2025 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host-side unit tests for the VirtIO block driver (probe + capacity).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test
//!
//! TEST_SCOPE:
//!   - Probe behavior
//!   - Capacity report behavior
//!
//! TEST_SCENARIOS:
//!   - capacity_and_probe(): Probe and capacity are computed
//!
//! DEPENDENCIES:
//!   - nexus_hal::{Bus, DmaBuffer}: test stubs
//!   - storage_virtio_blk::VirtioBlk: driver under test
//!
//! ADR: docs/architecture/01-neuron-kernel.md

use nexus_hal::{Bus, DmaBuffer};
use storage_virtio_blk::{
    DeviceInfo, VirtioBlk, VirtioError, VIRTIO_DEVICE_ID_BLK, VIRTIO_MMIO_MAGIC,
    VIRTIO_MMIO_VERSION_MODERN,
};

struct BusStub;

impl Bus for BusStub {
    fn read(&self, addr: usize) -> u32 {
        match addr {
            0x000 => VIRTIO_MMIO_MAGIC,
            0x004 => VIRTIO_MMIO_VERSION_MODERN,
            0x008 => VIRTIO_DEVICE_ID_BLK,
            0x00c => 0x1234,
            0x100 => 0,
            0x104 => 1,
            _ => 0,
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
fn capacity_and_probe() {
    let blk = VirtioBlk::new(BusStub);
    assert_eq!(
        blk.probe(),
        Ok(DeviceInfo {
            version: VIRTIO_MMIO_VERSION_MODERN,
            device_id: VIRTIO_DEVICE_ID_BLK,
            vendor_id: 0x1234,
        })
    );
    assert_eq!(blk.capacity_sectors(), 1_u64 << 32);
}

#[test]
fn read_block_not_implemented() {
    let blk = VirtioBlk::new(BusStub);
    let mut buf = BufStub;
    assert_eq!(blk.read_block(&mut buf), Err(VirtioError::Unsupported));
    assert_eq!(buf.len(), 512);
}
