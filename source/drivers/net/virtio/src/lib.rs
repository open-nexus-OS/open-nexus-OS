// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! CONTEXT: VirtIO net (virtio-mmio) bring-up driver (userspace-first)
//! OWNERS: @runtime
//! STATUS: In Progress
//! API_STABILITY: Unstable (bring-up)
//! TEST_COVERAGE: basic probe unit tests (host); OS/QEMU proof via selftest markers
//!
//! PUBLIC API:
//! - `VirtioNetMmio`: minimal virtio-mmio net device control plane (probe + queue programming)
//!
//! NOTE:
//! - This crate is intentionally minimal and polling-oriented for bring-up.
//! - Data-plane integration (smoltcp Device) lives in userspace until stabilized.

use nexus_hal::Bus;

/// VirtIO MMIO magic ("virt" LE).
pub const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976;
/// VirtIO MMIO legacy version.
pub const VIRTIO_MMIO_VERSION_LEGACY: u32 = 1;
/// VirtIO MMIO modern version.
pub const VIRTIO_MMIO_VERSION_MODERN: u32 = 2;
/// VirtIO device id for network cards.
pub const VIRTIO_DEVICE_ID_NET: u32 = 1;

// VirtIO MMIO register offsets (bytes).
const REG_MAGIC: usize = 0x000;
const REG_VERSION: usize = 0x004;
const REG_DEVICE_ID: usize = 0x008;
const REG_VENDOR_ID: usize = 0x00c;
const REG_DEVICE_FEATURES: usize = 0x010;
const REG_DEVICE_FEATURES_SEL: usize = 0x014;
const REG_DRIVER_FEATURES: usize = 0x020;
const REG_DRIVER_FEATURES_SEL: usize = 0x024;
const REG_GUEST_PAGE_SIZE: usize = 0x028; // legacy only
const REG_QUEUE_SEL: usize = 0x030;
const REG_QUEUE_NUM_MAX: usize = 0x034;
const REG_QUEUE_NUM: usize = 0x038;
const REG_QUEUE_ALIGN: usize = 0x03c; // legacy only
const REG_QUEUE_PFN: usize = 0x040; // legacy only
const REG_QUEUE_READY: usize = 0x044;
const REG_QUEUE_NOTIFY: usize = 0x050;
const REG_STATUS: usize = 0x070;

const REG_QUEUE_DESC_LOW: usize = 0x080;
const REG_QUEUE_DESC_HIGH: usize = 0x084;
const REG_QUEUE_DRIVER_LOW: usize = 0x090;
const REG_QUEUE_DRIVER_HIGH: usize = 0x094;
const REG_QUEUE_DEVICE_LOW: usize = 0x0a0;
const REG_QUEUE_DEVICE_HIGH: usize = 0x0a4;

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
    NotNetDevice,
    QueueUnavailable,
    QueueTooSmall,
    DeviceRejectedFeatures,
}

/// A minimal virtio-mmio net device wrapper.
///
/// `bus.read/write(offset)` addresses are interpreted as **MMIO register offsets**.
pub struct VirtioNetMmio<B: Bus> {
    bus: B,
}

impl<B: Bus> VirtioNetMmio<B> {
    pub fn new(bus: B) -> Self {
        Self { bus }
    }

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
        if device_id != VIRTIO_DEVICE_ID_NET {
            return Err(VirtioError::NotNetDevice);
        }
        let vendor_id = self.bus.read(REG_VENDOR_ID);
        Ok(DeviceInfo {
            version,
            device_id,
            vendor_id,
        })
    }

    /// Resets the device status to 0.
    pub fn reset(&self) {
        self.bus.write(REG_STATUS, 0);
    }

    /// Minimal feature negotiation: accept feature bits exactly as provided.
    ///
    /// Bring-up policy: caller typically passes 0 (disable all optional features).
    pub fn negotiate_features(&self, driver_features: u64) -> Result<(), VirtioError> {
        // ACK + DRIVER
        self.bus
            .write(REG_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        // Read device features (two 32-bit windows).
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

        // FEATURES_OK
        let st = self.bus.read(REG_STATUS);
        self.bus.write(REG_STATUS, st | STATUS_FEATURES_OK);

        // Device may clear FEATURES_OK if it rejects.
        let st2 = self.bus.read(REG_STATUS);
        if st2 & STATUS_FEATURES_OK == 0 {
            self.bus.write(REG_STATUS, st2 | STATUS_FAILED);
            return Err(VirtioError::DeviceRejectedFeatures);
        }
        Ok(())
    }

    /// Programs a queue's descriptor/avail/used addresses (physical) and marks it READY.
    ///
    /// Caller must ensure memory is DMA-safe and correctly aligned.
    pub fn setup_queue(&self, index: u32, cfg: &QueueSetup) -> Result<(), VirtioError> {
        self.bus.write(REG_QUEUE_SEL, index);
        let max = self.bus.read(REG_QUEUE_NUM_MAX);
        if max == 0 {
            return Err(VirtioError::QueueUnavailable);
        }
        if (cfg.size as u32) > max {
            return Err(VirtioError::QueueTooSmall);
        }
        self.bus.write(REG_QUEUE_NUM, cfg.size as u32);

        let version = self.bus.read(REG_VERSION);
        if version == VIRTIO_MMIO_VERSION_MODERN {
            write_u64_mmio(&self.bus, REG_QUEUE_DESC_LOW, cfg.desc_paddr);
            write_u64_mmio(&self.bus, REG_QUEUE_DRIVER_LOW, cfg.avail_paddr);
            write_u64_mmio(&self.bus, REG_QUEUE_DEVICE_LOW, cfg.used_paddr);
            self.bus.write(REG_QUEUE_READY, 1);
            Ok(())
        } else if version == VIRTIO_MMIO_VERSION_LEGACY {
            // Legacy virtio-mmio requires the guest page size register to interpret PFNs.
            self.bus.write(REG_GUEST_PAGE_SIZE, 4096);
            // Legacy virtio-mmio: a single queue PFN points at the start of a combined
            // virtqueue layout, aligned to queue_align.
            //
            // Use the minimum required alignment during bring-up to avoid requiring multi-page
            // virtqueue layouts in userspace.
            self.bus.write(REG_QUEUE_ALIGN, 4);
            let pfn = (cfg.desc_paddr >> 12) as u32;
            self.bus.write(REG_QUEUE_PFN, pfn);
            Ok(())
        } else {
            Err(VirtioError::UnsupportedVersion)
        }
    }

    pub fn set_driver_ok(&self) {
        let st = self.bus.read(REG_STATUS);
        self.bus.write(REG_STATUS, st | STATUS_DRIVER_OK);
    }

    pub fn notify_queue(&self, queue_index: u32) {
        self.bus.write(REG_QUEUE_NOTIFY, queue_index);
    }
}

fn write_u64_mmio<B: Bus>(bus: &B, low_reg: usize, value: u64) {
    let lo = (value & 0xffff_ffff) as u32;
    let hi = (value >> 32) as u32;
    bus.write(low_reg, lo);
    bus.write(low_reg + 4, hi);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub version: u32,
    pub device_id: u32,
    pub vendor_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueSetup {
    pub size: u16,
    pub desc_paddr: u64,
    pub avail_paddr: u64,
    pub used_paddr: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBus {
        regs: [u32; 0x200 / 4],
    }

    impl MockBus {
        fn new() -> Self {
            Self { regs: [0; 0x200 / 4] }
        }
        fn set(&mut self, off: usize, v: u32) {
            self.regs[off / 4] = v;
        }
    }

    impl Bus for MockBus {
        fn read(&self, addr: usize) -> u32 {
            self.regs[addr / 4]
        }
        fn write(&self, _addr: usize, _value: u32) {}
    }

    #[test]
    fn probe_rejects_bad_magic() {
        let mut bus = MockBus::new();
        bus.set(REG_MAGIC, 0);
        bus.set(REG_VERSION, VIRTIO_MMIO_VERSION_MODERN);
        bus.set(REG_DEVICE_ID, VIRTIO_DEVICE_ID_NET);
        let dev = VirtioNetMmio::new(bus);
        assert_eq!(dev.probe(), Err(VirtioError::BadMagic));
    }
}
