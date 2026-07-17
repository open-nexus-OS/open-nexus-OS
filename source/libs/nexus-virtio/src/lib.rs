// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared **virtio-mmio transport HAL** — the device-agnostic half every
//! virtio driver re-implemented by hand (the same ~50-line register map + ~30-line
//! feature negotiation + queue setup, copy-pasted across virtio-blk / virtio-net /
//! virtio-rng / virtio-input). This crate owns it once: the register map, the
//! VirtIO device-init handshake (reset → ACK → DRIVER → features → FEATURES_OK →
//! queue setup → DRIVER_OK), the split-virtqueue ring structs, and queue
//! programming. A driver then shrinks to its **device id + config registers +
//! device-specific command logic** (Gate 3 of the gfx/driver idealstruktur track,
//! TRACK-DRIVERS-ACCELERATORS).
//!
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host golden tests (`cargo test -p nexus-virtio`) over a MockBus.
//!
//! DESIGN: generic over [`nexus_hal::Bus`] (the `read`/`write` u32 MMIO trait), so
//! it is `forbid(unsafe_code)` — the raw-pointer MMIO lives in each driver's tiny
//! `Bus` impl, the host tests drive a mock. Supports both legacy (v1) and modern
//! (v2) virtio-mmio.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

use nexus_hal::Bus;

// ── Identity ─────────────────────────────────────────────────────────────────

/// VirtIO MMIO magic ("virt" LE).
pub const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976;
/// Legacy virtio-mmio version (v1).
pub const VIRTIO_MMIO_VERSION_LEGACY: u32 = 1;
/// Modern virtio-mmio version (v2).
pub const VIRTIO_MMIO_VERSION_MODERN: u32 = 2;

// ── Register offsets (bytes) ─────────────────────────────────────────────────

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

/// First byte of the device-specific configuration space. Drivers read their
/// config registers at `REG_CONFIG_BASE + offset` via [`VirtioMmio::read_config`].
pub const REG_CONFIG_BASE: usize = 0x100;

// ── Status bits (VirtIO 1.0) ─────────────────────────────────────────────────

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;
const STATUS_FAILED: u32 = 128;

// ── Split-virtqueue ring layout ──────────────────────────────────────────────

/// Descriptor chains to the `next` descriptor.
pub const VIRTQ_DESC_F_NEXT: u16 = 1;
/// Descriptor is device-writable (else driver-writable).
pub const VIRTQ_DESC_F_WRITE: u16 = 2;

/// A split-virtqueue descriptor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

/// One entry of the used ring.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VqUsedElem {
    pub id: u32,
    pub len: u32,
}

/// The available ring (`N` = queue size).
#[repr(C)]
pub struct VqAvail<const N: usize> {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; N],
    pub used_event: u16,
}

/// The used ring (`N` = queue size).
#[repr(C)]
pub struct VqUsed<const N: usize> {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VqUsedElem; N],
    pub avail_event: u16,
}

// ── Types ────────────────────────────────────────────────────────────────────

/// Minimal virtio-mmio probe/init failure modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtioError {
    /// `REG_MAGIC` was not "virt".
    BadMagic,
    /// `REG_VERSION` was neither legacy (1) nor modern (2).
    UnsupportedVersion,
    /// `REG_DEVICE_ID` did not match the expected device class.
    WrongDeviceId,
    /// The device cleared FEATURES_OK — it rejected the negotiated feature set.
    DeviceRejectedFeatures,
    /// The requested operation is not supported (e.g. queue too large).
    Unsupported,
}

/// MMIO identity read during [`VirtioMmio::probe`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub version: u32,
    pub device_id: u32,
    pub vendor_id: u32,
}

/// Physical addresses + size for programming a queue ([`VirtioMmio::setup_queue`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueSetup {
    pub size: u16,
    pub desc_paddr: u64,
    pub avail_paddr: u64,
    pub used_paddr: u64,
}

// ── Transport ────────────────────────────────────────────────────────────────

/// A virtio-mmio device transport over a [`Bus`]. Owns only the bus handle; the
/// device-init handshake and queue programming are its methods.
pub struct VirtioMmio<B: Bus> {
    bus: B,
}

impl<B: Bus> VirtioMmio<B> {
    #[must_use]
    pub fn new(bus: B) -> Self {
        Self { bus }
    }

    /// Borrow the underlying bus (for device-specific register access).
    #[must_use]
    pub fn bus(&self) -> &B {
        &self.bus
    }

    /// Read a device-config register at `REG_CONFIG_BASE + offset`.
    #[must_use]
    pub fn read_config(&self, offset: usize) -> u32 {
        self.bus.read(REG_CONFIG_BASE + offset)
    }

    /// Validate MMIO identity against `expected_device_id` and return the info.
    pub fn probe(&self, expected_device_id: u32) -> Result<DeviceInfo, VirtioError> {
        let magic = self.bus.read(REG_MAGIC);
        if magic != VIRTIO_MMIO_MAGIC {
            return Err(VirtioError::BadMagic);
        }
        let version = self.bus.read(REG_VERSION);
        if version != VIRTIO_MMIO_VERSION_LEGACY && version != VIRTIO_MMIO_VERSION_MODERN {
            return Err(VirtioError::UnsupportedVersion);
        }
        let device_id = self.bus.read(REG_DEVICE_ID);
        if device_id != expected_device_id {
            return Err(VirtioError::WrongDeviceId);
        }
        let vendor_id = self.bus.read(REG_VENDOR_ID);
        Ok(DeviceInfo { version, device_id, vendor_id })
    }

    /// Reset the device (status register → 0).
    pub fn reset(&self) {
        self.bus.write(REG_STATUS, 0);
    }

    /// Run the feature-negotiation handshake, accepting `driver_features`, and
    /// return the **accepted** feature mask (the intersection with what the device
    /// offered). Handles both legacy (v1) and modern (v2) virtio-mmio.
    pub fn negotiate_features(&self, driver_features: u64) -> Result<u64, VirtioError> {
        let version = self.bus.read(REG_VERSION);

        // ACK + DRIVER
        self.bus.write(REG_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        if version == VIRTIO_MMIO_VERSION_MODERN {
            // Modern: 64-bit features via selector registers.
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

            // FEATURES_OK (modern only).
            let st = self.bus.read(REG_STATUS);
            self.bus.write(REG_STATUS, st | STATUS_FEATURES_OK);

            // Device may clear FEATURES_OK if it rejects.
            let st2 = self.bus.read(REG_STATUS);
            if st2 & STATUS_FEATURES_OK == 0 {
                self.bus.write(REG_STATUS, st2 | STATUS_FAILED);
                return Err(VirtioError::DeviceRejectedFeatures);
            }
            Ok(accept)
        } else {
            // Legacy: single 32-bit feature register, no FEATURES_OK step.
            let dev_lo = self.bus.read(REG_DEVICE_FEATURES);
            let accept = (driver_features as u32) & dev_lo;
            self.bus.write(REG_DRIVER_FEATURES, accept);
            Ok(accept as u64)
        }
    }

    /// Mark the device DRIVER_OK after queue setup.
    pub fn driver_ok(&self) {
        let st = self.bus.read(REG_STATUS);
        self.bus.write(REG_STATUS, st | STATUS_DRIVER_OK);
    }

    /// Program a queue's descriptor/avail/used addresses (physical) and mark it
    /// READY. Caller ensures the memory is DMA-safe and correctly aligned.
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
            self.write_u64_pair(REG_QUEUE_DESC_LOW, REG_QUEUE_DESC_HIGH, cfg.desc_paddr);
            self.write_u64_pair(REG_QUEUE_DRIVER_LOW, REG_QUEUE_DRIVER_HIGH, cfg.avail_paddr);
            self.write_u64_pair(REG_QUEUE_DEVICE_LOW, REG_QUEUE_DEVICE_HIGH, cfg.used_paddr);
            self.bus.write(REG_QUEUE_READY, 1);
            Ok(())
        } else if version == VIRTIO_MMIO_VERSION_LEGACY {
            // Legacy virtio-mmio needs the guest page size to interpret PFNs.
            self.bus.write(REG_GUEST_PAGE_SIZE, 4096);
            self.bus.write(REG_QUEUE_ALIGN, 4);
            let pfn = (cfg.desc_paddr >> 12) as u32;
            self.bus.write(REG_QUEUE_PFN, pfn);
            Ok(())
        } else {
            Err(VirtioError::UnsupportedVersion)
        }
    }

    /// Notify the device that `queue_index` has new available buffers.
    pub fn notify_queue(&self, queue_index: u32) {
        self.bus.write(REG_QUEUE_NOTIFY, queue_index);
    }

    fn write_u64_pair(&self, lo: usize, hi: usize, value: u64) {
        self.bus.write(lo, (value & 0xffff_ffff) as u32);
        self.bus.write(hi, (value >> 32) as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::RefCell;
    use std::vec::Vec;

    /// A mock bus that answers probe reads and records writes.
    struct MockBus {
        device_id: u32,
        version: u32,
        writes: RefCell<Vec<(usize, u32)>>,
    }

    impl MockBus {
        fn new(device_id: u32, version: u32) -> Self {
            Self { device_id, version, writes: RefCell::new(Vec::new()) }
        }
    }

    impl Bus for MockBus {
        fn read(&self, addr: usize) -> u32 {
            match addr {
                REG_MAGIC => VIRTIO_MMIO_MAGIC,
                REG_VERSION => self.version,
                REG_DEVICE_ID => self.device_id,
                REG_VENDOR_ID => 0x1234,
                REG_DEVICE_FEATURES => 0xFFFF_FFFF,
                REG_QUEUE_NUM_MAX => 256,
                // After we set FEATURES_OK, report it back so negotiation succeeds.
                REG_STATUS => {
                    if self
                        .writes
                        .borrow()
                        .iter()
                        .any(|(a, v)| *a == REG_STATUS && v & STATUS_FEATURES_OK != 0)
                    {
                        STATUS_FEATURES_OK
                    } else {
                        0
                    }
                }
                _ => 0,
            }
        }
        fn write(&self, addr: usize, value: u32) {
            self.writes.borrow_mut().push((addr, value));
        }
    }

    #[test]
    fn probe_validates_identity() {
        let dev = VirtioMmio::new(MockBus::new(2, VIRTIO_MMIO_VERSION_MODERN));
        assert_eq!(dev.probe(2).map(|i| i.device_id), Ok(2));
        assert_eq!(dev.probe(99), Err(VirtioError::WrongDeviceId));
    }

    #[test]
    fn modern_negotiation_accepts_offered_features() {
        let dev = VirtioMmio::new(MockBus::new(1, VIRTIO_MMIO_VERSION_MODERN));
        assert_eq!(dev.negotiate_features(0x1), Ok(0x1));
        // Driver wrote the accepted low word back.
        assert!(dev
            .bus()
            .writes
            .borrow()
            .iter()
            .any(|(a, v)| *a == REG_DRIVER_FEATURES && *v == 1));
    }

    #[test]
    fn setup_queue_rejects_oversized_and_programs_modern() {
        let dev = VirtioMmio::new(MockBus::new(1, VIRTIO_MMIO_VERSION_MODERN));
        let big = QueueSetup { size: 1024, desc_paddr: 0, avail_paddr: 0, used_paddr: 0 };
        assert_eq!(dev.setup_queue(0, &big), Err(VirtioError::Unsupported));
        let ok =
            QueueSetup { size: 64, desc_paddr: 0x1000, avail_paddr: 0x2000, used_paddr: 0x3000 };
        assert_eq!(dev.setup_queue(0, &ok), Ok(()));
        assert!(dev.bus().writes.borrow().iter().any(|(a, _)| *a == REG_QUEUE_READY));
    }
}
