// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! CONTEXT: VirtIO block driver for storage operations
//! OWNERS: @runtime
//! STATUS: In Progress
//! API_STABILITY: Unstable (bring-up)
//! TEST_COVERAGE: 2 unit tests (probe + capacity)
//!
//! PUBLIC API:
//!   - VirtioBlk: Block driver implementation
//!   - probe(): Validate MMIO identity
//!   - capacity_sectors(): Get storage capacity in sectors
//!   - read_block(): Read data block (not yet implemented)
//!   - write_block(): Write data block (not yet implemented)
//!
//! DEPENDENCIES:
//!   - nexus-hal::{Bus, DmaBuffer}: Hardware abstraction layer
//!
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use nexus_hal::{Bus, DmaBuffer};

/// Pure request-ring logic (free-list + used-ring reclaim) — host-tested.
pub mod ring;

/// VirtIO MMIO magic ("virt" LE).
pub const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976;
/// VirtIO MMIO legacy version.
pub const VIRTIO_MMIO_VERSION_LEGACY: u32 = 1;
/// VirtIO MMIO modern version.
pub const VIRTIO_MMIO_VERSION_MODERN: u32 = 2;
/// VirtIO device id for block devices.
pub const VIRTIO_DEVICE_ID_BLK: u32 = 2;

// VirtIO MMIO register offsets (bytes).
const REG_MAGIC: usize = 0x000;
const REG_VERSION: usize = 0x004;
const REG_DEVICE_ID: usize = 0x008;
const REG_VENDOR_ID: usize = 0x00c;
const REG_DEVICE_FEATURES: usize = 0x010;
const REG_DEVICE_FEATURES_SEL: usize = 0x014;
const REG_DRIVER_FEATURES: usize = 0x020;
const REG_DRIVER_FEATURES_SEL: usize = 0x024;
const REG_STATUS: usize = 0x070;

const REG_QUEUE_SEL: usize = 0x030;
const REG_QUEUE_NUM_MAX: usize = 0x034;
const REG_QUEUE_NUM: usize = 0x038;
const REG_GUEST_PAGE_SIZE: usize = 0x028; // legacy only
const REG_QUEUE_ALIGN: usize = 0x03c; // legacy only
const REG_QUEUE_PFN: usize = 0x040; // legacy only
const REG_QUEUE_READY: usize = 0x044;
const REG_QUEUE_NOTIFY: usize = 0x050;

const REG_QUEUE_DESC_LOW: usize = 0x080;
const REG_QUEUE_DESC_HIGH: usize = 0x084;
const REG_QUEUE_DRIVER_LOW: usize = 0x090;
const REG_QUEUE_DRIVER_HIGH: usize = 0x094;
const REG_QUEUE_DEVICE_LOW: usize = 0x0a0;
const REG_QUEUE_DEVICE_HIGH: usize = 0x0a4;

const REG_CONFIG_BASE: usize = 0x100;
const REG_CONFIG_CAPACITY_LOW: usize = REG_CONFIG_BASE;
const REG_CONFIG_CAPACITY_HIGH: usize = REG_CONFIG_BASE + 0x04;

// Status bits (VirtIO 1.0).
const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;
const STATUS_FAILED: u32 = 128;

/// Errors for minimal virtio-mmio probe/init.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtioError {
    BadMagic,
    UnsupportedVersion,
    NotBlockDevice,
    DeviceRejectedFeatures,
    Unsupported,
}

/// Queue configuration for setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueSetup {
    pub size: u16,
    pub desc_paddr: u64,
    pub avail_paddr: u64,
    pub used_paddr: u64,
}

/// Device identity information.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub version: u32,
    pub device_id: u32,
    pub vendor_id: u32,
}

pub struct VirtioBlk<B: Bus> {
    bus: B,
}

impl<B: Bus> VirtioBlk<B> {
    pub fn new(bus: B) -> Self {
        Self { bus }
    }

    /// Validate MMIO identity and return device info.
    pub fn probe(&self) -> Result<DeviceInfo, VirtioError> {
        let magic = self.bus.read(REG_MAGIC);
        if magic != VIRTIO_MMIO_MAGIC {
            return Err(VirtioError::BadMagic);
        }
        let version = self.bus.read(REG_VERSION);
        if version != VIRTIO_MMIO_VERSION_LEGACY && version != VIRTIO_MMIO_VERSION_MODERN {
            return Err(VirtioError::UnsupportedVersion);
        }
        let device_id = self.bus.read(REG_DEVICE_ID);
        if device_id != VIRTIO_DEVICE_ID_BLK {
            return Err(VirtioError::NotBlockDevice);
        }
        let vendor_id = self.bus.read(REG_VENDOR_ID);
        Ok(DeviceInfo { version, device_id, vendor_id })
    }

    /// Resets the device status to 0.
    pub fn reset(&self) {
        self.bus.write(REG_STATUS, 0);
    }

    /// Minimal feature negotiation: accept feature bits exactly as provided.
    /// Handles both legacy (v1) and modern (v2) virtio-mmio.
    pub fn negotiate_features(&self, driver_features: u64) -> Result<(), VirtioError> {
        let version = self.bus.read(REG_VERSION);

        // ACK + DRIVER
        self.bus.write(REG_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        if version == VIRTIO_MMIO_VERSION_MODERN {
            // Modern: 64-bit features via selector registers
            self.bus.write(REG_DEVICE_FEATURES_SEL, 0);
            let dev_lo = self.bus.read(REG_DEVICE_FEATURES);
            self.bus.write(REG_DEVICE_FEATURES_SEL, 1);
            let dev_hi = self.bus.read(REG_DEVICE_FEATURES);
            let dev = (dev_lo as u64) | ((dev_hi as u64) << 32);

            let accept = driver_features & dev;
            let out_lo = (accept & 0xffff_ffff) as u32;
            let out_hi = (accept >> 32) as u32;
            self.bus.write(REG_DRIVER_FEATURES_SEL, 0);
            self.bus.write(REG_DRIVER_FEATURES, out_lo);
            self.bus.write(REG_DRIVER_FEATURES_SEL, 1);
            self.bus.write(REG_DRIVER_FEATURES, out_hi);

            // FEATURES_OK (modern only)
            let st = self.bus.read(REG_STATUS);
            self.bus.write(REG_STATUS, st | STATUS_FEATURES_OK);

            // Device may clear FEATURES_OK if it rejects.
            let st2 = self.bus.read(REG_STATUS);
            if st2 & STATUS_FEATURES_OK == 0 {
                self.bus.write(REG_STATUS, st2 | STATUS_FAILED);
                return Err(VirtioError::DeviceRejectedFeatures);
            }
        } else {
            // Legacy: single 32-bit feature register, no FEATURES_OK step
            let dev_lo = self.bus.read(REG_DEVICE_FEATURES);
            let accept = (driver_features as u32) & dev_lo;
            self.bus.write(REG_DRIVER_FEATURES, accept);
            // Legacy doesn't have FEATURES_OK; proceed directly
        }
        Ok(())
    }

    /// Marks the device DRIVER_OK after queue setup.
    pub fn driver_ok(&self) {
        let st = self.bus.read(REG_STATUS);
        self.bus.write(REG_STATUS, st | STATUS_DRIVER_OK);
    }

    /// Return capacity in 512-byte sectors.
    pub fn capacity_sectors(&self) -> u64 {
        let low = self.bus.read(REG_CONFIG_CAPACITY_LOW) as u64;
        let high = self.bus.read(REG_CONFIG_CAPACITY_HIGH) as u64;
        (high << 32) | low
    }

    /// Return capacity in bytes (sector size = 512).
    pub fn capacity_bytes(&self) -> u64 {
        self.capacity_sectors().saturating_mul(512)
    }

    /// Programs a queue's descriptor/avail/used addresses (physical) and marks it READY.
    ///
    /// Caller must ensure memory is DMA-safe and correctly aligned.
    pub fn setup_queue(&self, index: u32, cfg: &QueueSetup) -> Result<(), VirtioError> {
        self.bus.write(REG_QUEUE_SEL, index);
        let max = self.bus.read(REG_QUEUE_NUM_MAX);
        if max == 0 {
            return Err(VirtioError::Unsupported);
        }
        if (cfg.size as u32) > max {
            return Err(VirtioError::Unsupported);
        }
        self.bus.write(REG_QUEUE_NUM, cfg.size as u32);

        let version = self.bus.read(REG_VERSION);
        if version == VIRTIO_MMIO_VERSION_MODERN {
            write_u64_mmio_pair(&self.bus, REG_QUEUE_DESC_LOW, REG_QUEUE_DESC_HIGH, cfg.desc_paddr);
            write_u64_mmio_pair(
                &self.bus,
                REG_QUEUE_DRIVER_LOW,
                REG_QUEUE_DRIVER_HIGH,
                cfg.avail_paddr,
            );
            write_u64_mmio_pair(
                &self.bus,
                REG_QUEUE_DEVICE_LOW,
                REG_QUEUE_DEVICE_HIGH,
                cfg.used_paddr,
            );
            self.bus.write(REG_QUEUE_READY, 1);
            Ok(())
        } else if version == VIRTIO_MMIO_VERSION_LEGACY {
            // Legacy virtio-mmio requires the guest page size register to interpret PFNs.
            self.bus.write(REG_GUEST_PAGE_SIZE, 4096);
            self.bus.write(REG_QUEUE_ALIGN, 4);
            let pfn = (cfg.desc_paddr >> 12) as u32;
            self.bus.write(REG_QUEUE_PFN, pfn);
            Ok(())
        } else {
            Err(VirtioError::UnsupportedVersion)
        }
    }

    pub fn notify_queue(&self, queue_index: u32) {
        self.bus.write(REG_QUEUE_NOTIFY, queue_index);
    }

    pub fn read_block<T: DmaBuffer>(&self, _buffer: &mut T) -> Result<(), VirtioError> {
        Err(VirtioError::Unsupported)
    }

    pub fn write_block<T: DmaBuffer>(&self, _buffer: &T) -> Result<(), VirtioError> {
        Err(VirtioError::Unsupported)
    }
}

fn write_u64_mmio_pair<B: Bus>(bus: &B, lo: usize, hi: usize, value: u64) {
    let lo_v = (value & 0xffff_ffff) as u32;
    let hi_v = (value >> 32) as u32;
    bus.write(lo, lo_v);
    bus.write(hi, hi_v);
}

/// Virtio-blk MMIO backend v2 (os-lite) — see `mmio.rs`.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub mod mmio;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use mmio::{VirtioBlkMmio, MAX_RUN_BYTES};

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_hal::{Bus, DmaBuffer};

    struct MockBus;

    impl Bus for MockBus {
        fn read(&self, addr: usize) -> u32 {
            match addr {
                REG_MAGIC => VIRTIO_MMIO_MAGIC,
                REG_VERSION => VIRTIO_MMIO_VERSION_MODERN,
                REG_DEVICE_ID => VIRTIO_DEVICE_ID_BLK,
                REG_VENDOR_ID => 0x1234,
                REG_CONFIG_CAPACITY_LOW => 0x0000_0000,
                REG_CONFIG_CAPACITY_HIGH => 0x0000_0001,
                _ => 0,
            }
        }

        fn write(&self, _addr: usize, _value: u32) {}
    }

    struct MockBuf;

    impl DmaBuffer for MockBuf {
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
    fn capacity_combines_high_low() {
        let blk = VirtioBlk::new(MockBus);
        assert_eq!(blk.capacity_sectors(), 1u64 << 32);
    }

    #[test]
    fn read_block_stubs() {
        let blk = VirtioBlk::new(MockBus);
        let mut buf = MockBuf;
        assert_eq!(blk.read_block(&mut buf), Err(VirtioError::Unsupported));
        assert_eq!(buf.len(), 512);
    }
}
