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

// =============================================================================
// Virtio-blk MMIO backend (os-lite)
// =============================================================================

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod mmio_backend {
    use core::mem::size_of;
    use core::sync::atomic::{fence, Ordering};

    use super::{
        QueueSetup, VirtioBlk, VirtioError, REG_QUEUE_NUM_MAX, REG_QUEUE_PFN, REG_QUEUE_SEL,
        REG_STATUS, VIRTIO_DEVICE_ID_BLK, VIRTIO_MMIO_MAGIC, VIRTIO_MMIO_VERSION_LEGACY,
        VIRTIO_MMIO_VERSION_MODERN,
    };
    use nexus_abi::{
        cap_query, debug_putc, mmio_map, nsec, vmo_create, vmo_map_page_sys, AbiError, CapQuery,
    };
    use nexus_hal::Bus;

    const VIRTQ_DESC_F_NEXT: u16 = 1;
    const VIRTQ_DESC_F_WRITE: u16 = 2;

    const VIRTIO_BLK_T_IN: u32 = 0;
    const VIRTIO_BLK_T_OUT: u32 = 1;
    const VIRTIO_BLK_T_FLUSH: u32 = 4;
    const VIRTIO_BLK_S_OK: u8 = 0;
    const VIRTIO_F_VERSION_1: u64 = 32;
    const VIRTIO_BLK_F_FLUSH: u64 = 9;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct VqDesc {
        addr: u64,
        len: u32,
        flags: u16,
        next: u16,
    }

    #[repr(C)]
    struct VqAvail<const N: usize> {
        flags: u16,
        idx: u16,
        ring: [u16; N],
        used_event: u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct VqUsedElem {
        id: u32,
        len: u32,
    }

    #[repr(C)]
    struct VqUsed<const N: usize> {
        flags: u16,
        idx: u16,
        ring: [VqUsedElem; N],
        avail_event: u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct BlkReq {
        req_type: u32,
        reserved: u32,
        sector: u64,
    }

    struct MmioBus {
        base: usize,
    }

    impl Bus for MmioBus {
        fn read(&self, addr: usize) -> u32 {
            unsafe { core::ptr::read_volatile((self.base + addr) as *const u32) }
        }
        fn write(&self, addr: usize, value: u32) {
            unsafe { core::ptr::write_volatile((self.base + addr) as *mut u32, value) }
        }
    }

    fn align4(x: usize) -> usize {
        (x + 3) & !3usize
    }

    fn mmio_map_ok(mmio_cap_slot: u32, va: usize, off: usize) -> Result<(), VirtioError> {
        match mmio_map(mmio_cap_slot, va, off) {
            Ok(()) => Ok(()),
            Err(AbiError::InvalidArgument) => Ok(()),
            Err(_) => Err(VirtioError::Unsupported),
        }
    }

    fn emit_line(msg: &str) {
        for byte in msg.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
            let _ = debug_putc(byte);
        }
    }

    fn emit_byte(b: u8) {
        let _ = debug_putc(b);
    }

    fn emit_bytes(msg: &[u8]) {
        for &b in msg {
            let _ = debug_putc(b);
        }
    }

    fn emit_hex_u32(v: u32) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for i in (0..8).rev() {
            let nibble = ((v >> (i * 4)) & 0xF) as usize;
            let _ = debug_putc(HEX[nibble]);
        }
    }

    fn cap_query_base_len(slot: u32) -> Result<(u64, u64), VirtioError> {
        let mut info = CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        cap_query(slot, &mut info).map_err(|_| VirtioError::Unsupported)?;
        Ok((info.base, info.len))
    }

    /// Virtio-blk MMIO backend (single-queue, polling, legacy layout).
    pub struct VirtioBlkMmio {
        dev: VirtioBlk<MmioBus>,
        queue_len: usize,
        desc: *mut VqDesc,
        avail: *mut VqAvail<QUEUE_LEN>,
        used: *mut VqUsed<QUEUE_LEN>,
        last_used: core::cell::Cell<u16>,
        req_va: usize,
        req_pa: u64,
        data_va: usize,
        data_pa: u64,
        status_va: usize,
        status_pa: u64,
        capacity_sectors: u64,
        sector_size: u32,
    }

    const QUEUE_LEN: usize = 8;
    // Use different VA from virtio-net (0x2000_e000) to avoid any potential conflicts
    const MMIO_VA: usize = 0x2001_0000;
    const Q_MEM_VA: usize = 0x2008_0000;
    const Q_PAGES: usize = 1;
    const BUF_VA: usize = 0x2009_0000;
    const BUF_PAGES: usize = 1;

    impl VirtioBlkMmio {
        pub fn new(mmio_cap_slot: u32) -> Result<Self, VirtioError> {
            mmio_map_ok(mmio_cap_slot, MMIO_VA, 0)?;
            let magic = unsafe { core::ptr::read_volatile((MMIO_VA + 0x000) as *const u32) };
            if magic != VIRTIO_MMIO_MAGIC {
                return Err(VirtioError::BadMagic);
            }
            let device_id = unsafe { core::ptr::read_volatile((MMIO_VA + 0x008) as *const u32) };
            if device_id != VIRTIO_DEVICE_ID_BLK {
                return Err(VirtioError::NotBlockDevice);
            }
            let version = unsafe { core::ptr::read_volatile((MMIO_VA + 0x004) as *const u32) };
            if version != VIRTIO_MMIO_VERSION_LEGACY && version != VIRTIO_MMIO_VERSION_MODERN {
                return Err(VirtioError::UnsupportedVersion);
            }
            if version == VIRTIO_MMIO_VERSION_MODERN {
                emit_line("virtio-blk: mmio modern");
            } else {
                emit_line("virtio-blk: mmio legacy");
            }

            let dev = VirtioBlk::new(MmioBus { base: MMIO_VA });
            dev.probe()?;
            dev.reset();

            // For legacy devices, set GUEST_PAGE_SIZE early (before feature negotiation)
            // Some QEMU versions expect this to be set before any queue operations.
            use super::REG_GUEST_PAGE_SIZE;
            dev.bus.write(REG_GUEST_PAGE_SIZE, 4096);

            let driver_features = if version == VIRTIO_MMIO_VERSION_MODERN {
                (1u64 << VIRTIO_F_VERSION_1) | (1u64 << VIRTIO_BLK_F_FLUSH)
            } else {
                0
            };

            dev.negotiate_features(driver_features)?;

            // Queue memory.
            let q_vmo = vmo_create(Q_PAGES * 4096).map_err(|_| VirtioError::Unsupported)?;
            let flags = nexus_abi::page_flags::VALID
                | nexus_abi::page_flags::USER
                | nexus_abi::page_flags::READ
                | nexus_abi::page_flags::WRITE;
            for page in 0..Q_PAGES {
                let va = Q_MEM_VA + page * 4096;
                let off = page * 4096;
                vmo_map_page_sys(q_vmo, va, off, flags).map_err(|_| VirtioError::Unsupported)?;
            }
            let (q_base_pa, _q_len) = cap_query_base_len(q_vmo as u32)?;

            // Debug: show queue physical address
            emit_bytes(b"virtio-blk: q_pa=0x");
            emit_hex_u32((q_base_pa >> 32) as u32);
            emit_hex_u32(q_base_pa as u32);
            emit_bytes(b" pfn=0x");
            emit_hex_u32((q_base_pa >> 12) as u32);
            emit_byte(b'\n');

            let desc_bytes = size_of::<VqDesc>() * QUEUE_LEN;
            let avail_bytes = size_of::<VqAvail<QUEUE_LEN>>();
            let used_off = align4(desc_bytes + avail_bytes);

            let desc_va = Q_MEM_VA;
            let avail_va = Q_MEM_VA + desc_bytes;
            let used_va = Q_MEM_VA + used_off;

            // Zero the queue memory BEFORE setting up the queue and calling driver_ok.
            // The device starts using the queue as soon as driver_ok is called.
            unsafe { core::ptr::write_bytes(Q_MEM_VA as *mut u8, 0, Q_PAGES * 4096) };

            // Debug: check QUEUE_NUM_MAX before setup
            dev.bus.write(REG_QUEUE_SEL, 0);
            let q_max = dev.bus.read(REG_QUEUE_NUM_MAX);
            emit_bytes(b"virtio-blk: q_max=");
            emit_hex_u32(q_max);
            emit_byte(b'\n');

            dev.setup_queue(
                0,
                &QueueSetup {
                    size: QUEUE_LEN as u16,
                    desc_paddr: q_base_pa + 0,
                    avail_paddr: q_base_pa + desc_bytes as u64,
                    used_paddr: q_base_pa + used_off as u64,
                },
            )?;

            // Debug: verify queue configuration before driver_ok
            let status_before = dev.bus.read(REG_STATUS);
            emit_bytes(b"virtio-blk: status=");
            emit_hex_u32(status_before);
            emit_byte(b'\n');

            // Verify QUEUE_PFN was written correctly by reading it back
            dev.bus.write(REG_QUEUE_SEL, 0);
            let pfn_readback = dev.bus.read(REG_QUEUE_PFN);
            emit_bytes(b"virtio-blk: pfn_rb=");
            emit_hex_u32(pfn_readback);
            emit_byte(b'\n');

            // Debug: show queue memory layout
            emit_bytes(b"virtio-blk: q_layout desc=");
            emit_hex_u32(desc_va as u32);
            emit_bytes(b" avail=");
            emit_hex_u32(avail_va as u32);
            emit_bytes(b" used=");
            emit_hex_u32(used_va as u32);
            emit_byte(b'\n');

            dev.driver_ok();

            // Debug: verify driver_ok was accepted
            let status_after = dev.bus.read(REG_STATUS);
            emit_bytes(b"virtio-blk: status_ok=");
            emit_hex_u32(status_after);
            emit_byte(b'\n');

            // Initial queue kick to ensure QEMU's virtio-blk is ready
            dev.notify_queue(0);

            // Buffer memory (single page).
            let buf_vmo = vmo_create(BUF_PAGES * 4096).map_err(|_| VirtioError::Unsupported)?;
            for page in 0..BUF_PAGES {
                let va = BUF_VA + page * 4096;
                let off = page * 4096;
                vmo_map_page_sys(buf_vmo, va, off, flags).map_err(|_| VirtioError::Unsupported)?;
            }
            let (buf_base_pa, _buf_len) = cap_query_base_len(buf_vmo as u32)?;

            // Debug: show buffer physical address
            emit_bytes(b"virtio-blk: buf_pa=0x");
            emit_hex_u32((buf_base_pa >> 32) as u32);
            emit_hex_u32(buf_base_pa as u32);
            emit_byte(b'\n');

            let req_va = BUF_VA;
            let data_va = BUF_VA + 512;
            let status_va = BUF_VA + 1024;
            let req_pa = buf_base_pa + 0;
            let data_pa = buf_base_pa + 512;
            let status_pa = buf_base_pa + 1024;

            let capacity_sectors = dev.capacity_sectors();
            let sector_size = 512u32;

            // Debug: show device capacity
            emit_bytes(b"virtio-blk: cap=");
            emit_hex_u32((capacity_sectors >> 32) as u32);
            emit_hex_u32(capacity_sectors as u32);
            emit_bytes(b" sectors\n");

            let blk = Self {
                dev,
                queue_len: QUEUE_LEN,
                desc: desc_va as *mut VqDesc,
                avail: avail_va as *mut VqAvail<QUEUE_LEN>,
                used: used_va as *mut VqUsed<QUEUE_LEN>,
                last_used: core::cell::Cell::new(0),
                req_va,
                req_pa,
                data_va,
                data_pa,
                status_va,
                status_pa,
                capacity_sectors,
                sector_size,
            };

            // Give QEMU time to fully initialize the device after driver_ok
            for _ in 0..1000 {
                let _ = nexus_abi::yield_();
            }

            // Warm-up read to ensure the device is actually responding.
            // This catches early failures before the caller tries to use the device.
            let mut warmup_buf = [0u8; 512];
            if let Err(e) = blk.submit(VIRTIO_BLK_T_IN, 0, &mut warmup_buf) {
                emit_line("virtio-blk: warmup failed");
                return Err(e);
            }
            emit_line("virtio-blk: warmup ok");

            Ok(blk)
        }

        pub fn capacity_sectors(&self) -> u64 {
            self.capacity_sectors
        }

        pub fn sector_size(&self) -> u32 {
            self.sector_size
        }

        pub fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), VirtioError> {
            self.submit(VIRTIO_BLK_T_IN, block_idx, buf)
        }

        pub fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), VirtioError> {
            let mut tmp = [0u8; 512];
            tmp.copy_from_slice(&buf[..512]);
            self.submit(VIRTIO_BLK_T_OUT, block_idx, &mut tmp)
        }

        pub fn sync(&mut self) -> Result<(), VirtioError> {
            let mut dummy = [];
            self.submit(VIRTIO_BLK_T_FLUSH, 0, &mut dummy)
        }

        fn submit(&self, req_type: u32, sector: u64, data: &mut [u8]) -> Result<(), VirtioError> {
            let is_flush = req_type == VIRTIO_BLK_T_FLUSH;
            if !is_flush && data.len() < self.sector_size as usize {
                emit_line("virtio-blk: short buf");
                return Err(VirtioError::Unsupported);
            }
            if sector >= self.capacity_sectors {
                emit_line("virtio-blk: bad sector");
                return Err(VirtioError::Unsupported);
            }

            let req = BlkReq { req_type, reserved: 0, sector };
            unsafe {
                core::ptr::write_volatile(self.req_va as *mut BlkReq, req);
                core::ptr::write_bytes(self.status_va as *mut u8, 0, 1);
            }

            if req_type == VIRTIO_BLK_T_OUT {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr(),
                        self.data_va as *mut u8,
                        self.sector_size as usize,
                    );
                }
            }

            unsafe {
                // Use volatile writes for descriptors since device reads them
                if is_flush {
                    core::ptr::write_volatile(
                        self.desc.add(0),
                        VqDesc {
                            addr: self.req_pa,
                            len: size_of::<BlkReq>() as u32,
                            flags: VIRTQ_DESC_F_NEXT,
                            next: 1,
                        },
                    );
                    core::ptr::write_volatile(
                        self.desc.add(1),
                        VqDesc { addr: self.status_pa, len: 1, flags: VIRTQ_DESC_F_WRITE, next: 0 },
                    );
                } else {
                    core::ptr::write_volatile(
                        self.desc.add(0),
                        VqDesc {
                            addr: self.req_pa,
                            len: size_of::<BlkReq>() as u32,
                            flags: VIRTQ_DESC_F_NEXT,
                            next: 1,
                        },
                    );
                    core::ptr::write_volatile(
                        self.desc.add(1),
                        VqDesc {
                            addr: self.data_pa,
                            len: self.sector_size,
                            flags: if req_type == VIRTIO_BLK_T_IN {
                                VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE
                            } else {
                                VIRTQ_DESC_F_NEXT
                            },
                            next: 2,
                        },
                    );
                    core::ptr::write_volatile(
                        self.desc.add(2),
                        VqDesc { addr: self.status_pa, len: 1, flags: VIRTQ_DESC_F_WRITE, next: 0 },
                    );
                }

                let avail = &mut *self.avail;
                let idx = core::ptr::read_volatile(&avail.idx);
                // Use volatile write for the ring entry since device reads it
                core::ptr::write_volatile(&mut avail.ring[(idx as usize) % self.queue_len], 0);
                core::ptr::write_volatile(&mut avail.idx, idx.wrapping_add(1));
            }

            fence(Ordering::SeqCst);

            // Debug trace (very verbose): keep off in QEMU smoke for determinism and speed.
            const TRACE_IO: bool = false;
            if TRACE_IO {
                // Debug: show descriptor chain details
                unsafe {
                    let d0 = core::ptr::read_volatile(self.desc.add(0));
                    let d1 = core::ptr::read_volatile(self.desc.add(1));
                    let d2 = core::ptr::read_volatile(self.desc.add(2));
                    emit_bytes(b"virtio-blk: d0 addr=");
                    emit_hex_u32((d0.addr >> 32) as u32);
                    emit_hex_u32(d0.addr as u32);
                    emit_bytes(b" len=");
                    emit_hex_u32(d0.len);
                    emit_bytes(b" fl=");
                    emit_hex_u32(d0.flags as u32);
                    emit_byte(b'\n');

                    emit_bytes(b"virtio-blk: d1 addr=");
                    emit_hex_u32((d1.addr >> 32) as u32);
                    emit_hex_u32(d1.addr as u32);
                    emit_bytes(b" len=");
                    emit_hex_u32(d1.len);
                    emit_bytes(b" fl=");
                    emit_hex_u32(d1.flags as u32);
                    emit_byte(b'\n');

                    emit_bytes(b"virtio-blk: d2 addr=");
                    emit_hex_u32((d2.addr >> 32) as u32);
                    emit_hex_u32(d2.addr as u32);
                    emit_bytes(b" len=");
                    emit_hex_u32(d2.len);
                    emit_bytes(b" fl=");
                    emit_hex_u32(d2.flags as u32);
                    emit_byte(b'\n');
                }

                // Debug: show avail ring contents
                unsafe {
                    let avail = &*self.avail;
                    let flags = core::ptr::read_volatile(&avail.flags);
                    let idx = core::ptr::read_volatile(&avail.idx);
                    let ring0 = core::ptr::read_volatile(&avail.ring[0]);
                    emit_bytes(b"virtio-blk: avail flags=");
                    emit_hex_u32(flags as u32);
                    emit_bytes(b" idx=");
                    emit_hex_u32(idx as u32);
                    emit_bytes(b" ring[0]=");
                    emit_hex_u32(ring0 as u32);
                    emit_byte(b'\n');

                    let used = &*self.used;
                    let used_flags = core::ptr::read_volatile(&used.flags);
                    let used_idx = core::ptr::read_volatile(&used.idx);
                    emit_bytes(b"virtio-blk: used flags=");
                    emit_hex_u32(used_flags as u32);
                    emit_bytes(b" idx=");
                    emit_hex_u32(used_idx as u32);
                    emit_byte(b'\n');
                }
            }

            let last_before = self.last_used.get();

            // Additional memory barrier before notify to ensure all writes are visible
            fence(Ordering::SeqCst);
            core::sync::atomic::compiler_fence(Ordering::SeqCst);

            self.dev.notify_queue(0);

            let start = nsec().unwrap_or(0);
            let deadline = start.saturating_add(2_000_000_000);
            let mut poll_count = 0u32;
            loop {
                unsafe {
                    let used_idx = core::ptr::read_volatile(&(*self.used).idx);
                    if used_idx != self.last_used.get() {
                        self.last_used.set(used_idx);
                        break;
                    }
                }
                poll_count += 1;
                let now = nsec().unwrap_or(0);
                if now >= deadline {
                    emit_bytes(b"virtio-blk: timeout last=");
                    emit_hex_u32(last_before as u32);
                    emit_bytes(b" polls=");
                    emit_hex_u32(poll_count);
                    emit_byte(b'\n');
                    return Err(VirtioError::Unsupported);
                }
                let _ = nexus_abi::yield_();
            }

            if req_type == VIRTIO_BLK_T_IN {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        self.data_va as *const u8,
                        data.as_mut_ptr(),
                        self.sector_size as usize,
                    );
                }
            }

            let status = unsafe { core::ptr::read_volatile(self.status_va as *const u8) };
            if status != VIRTIO_BLK_S_OK {
                emit_bytes(b"virtio-blk: status err=");
                emit_hex_u32(status as u32);
                emit_byte(b'\n');
                return Err(VirtioError::Unsupported);
            }
            Ok(())
        }
    }
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use mmio_backend::VirtioBlkMmio;

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
