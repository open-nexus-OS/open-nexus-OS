// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxboot's own minimal POLLING virtio-blk reader (RFC-0089 §7;
//! ADR-0059 frozen scope). The userspace driver stack is syscall-bound and
//! not reusable pre-OS, so the loader speaks the transport directly:
//! probe the virtio-mmio windows, bring up ONE 8-entry queue (modern
//! transport ONLY — the harness runs force-legacy=off; a legacy window is
//! named loudly and refused, never half-driven), submit
//! 3-descriptor chains and poll the used ring with a bounded spin (wait-
//! loop doctrine: a dead device is an `Io` error, never a hang). Raw
//! memory is reached ONLY through the `arch` volatile helpers — this file
//! stays `deny(unsafe_code)` like the rest of the crate.
//! OWNERS: @security @runtime
//! STATUS: Experimental (TASK-0289 Phase A)
//! TEST_COVERAGE: flow logic host-tested against fixture disks; this
//!   transport is proven by the QEMU ladder at A4.
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

use core::cell::Cell;

use storage::{BlockDevice, BlockError};

use crate::arch;

const VIRTIO_MMIO_BASE: usize = 0x1000_1000;
const VIRTIO_MMIO_STRIDE: usize = 0x1000;
const VIRTIO_MMIO_SLOTS: usize = 8;

const REG_MAGIC: usize = 0x000;
const REG_VERSION: usize = 0x004;
const REG_DEVICE_ID: usize = 0x008;
const REG_DEVICE_FEATURES_SEL: usize = 0x014;
const REG_DEVICE_FEATURES: usize = 0x010;
const REG_DRIVER_FEATURES: usize = 0x020;
const REG_DRIVER_FEATURES_SEL: usize = 0x024;
const REG_QUEUE_SEL: usize = 0x030;
const REG_QUEUE_NUM_MAX: usize = 0x034;
const REG_QUEUE_NUM: usize = 0x038;
const REG_QUEUE_READY: usize = 0x044;
const REG_QUEUE_NOTIFY: usize = 0x050;
const REG_STATUS: usize = 0x070;
const REG_QUEUE_DESC_LOW: usize = 0x080;
const REG_QUEUE_DESC_HIGH: usize = 0x084;
const REG_QUEUE_DRIVER_LOW: usize = 0x090;
const REG_QUEUE_DRIVER_HIGH: usize = 0x094;
const REG_QUEUE_DEVICE_LOW: usize = 0x0a0;
const REG_QUEUE_DEVICE_HIGH: usize = 0x0a4;
const REG_CONFIG_CAPACITY_LOW: usize = 0x100;
const REG_CONFIG_CAPACITY_HIGH: usize = 0x104;

const VIRTIO_MAGIC: u32 = 0x7472_6976;
const VERSION_MODERN: u32 = 2;
const DEVICE_ID_BLK: u32 = 2;

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;
const VIRTIO_F_VERSION_1: u64 = 32;

const QUEUE_LEN: usize = 8;
const DESC_F_NEXT: u16 = 1;
const DESC_F_WRITE: u16 = 2;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_S_OK: u8 = 0;

const SECTOR: usize = 512;
/// One data descriptor per request; 64 sectors keeps the request count for
/// a 19 MB image around ~600 polls.
const MAX_RUN_SECTORS: usize = 64;
/// Bounded used-ring poll budget per request (TCG-safe headroom).
const POLL_SPINS: u32 = 200_000_000;

// Split-ring layout inside the single queue page (matches the proven
// TASK-0314 offsets: avail carries the trailing used_event u16).
const DESC_OFF: usize = 0;
const AVAIL_OFF: usize = 16 * QUEUE_LEN;
const AVAIL_BYTES: usize = 6 + 2 * QUEUE_LEN + 2;
const USED_OFF: usize = (AVAIL_OFF + AVAIL_BYTES + 3) & !3;

const REQ_HDR_OFF: usize = 0;
const REQ_STATUS_OFF: usize = 64;

/// The loader's one block device: queue index 0, three-descriptor chains,
/// interior mutability because `BlockDevice::read_block` takes `&self`.
pub struct VirtioDisk {
    base: usize,
    capacity_sectors: u64,
    avail_idx: Cell<u16>,
    used_seen: Cell<u16>,
}

impl VirtioDisk {
    /// Probes the virt machine's transport windows for the (single) blk
    /// device and brings its queue up. `None` = no disk — terminal.
    pub fn probe() -> Option<Self> {
        for slot in 0..VIRTIO_MMIO_SLOTS {
            let base = VIRTIO_MMIO_BASE + slot * VIRTIO_MMIO_STRIDE;
            if arch::mmio_read32(base + REG_MAGIC) != VIRTIO_MAGIC {
                continue;
            }
            if arch::mmio_read32(base + REG_DEVICE_ID) != DEVICE_ID_BLK {
                continue;
            }
            let version = arch::mmio_read32(base + REG_VERSION);
            if version != VERSION_MODERN {
                // The harness runs virtio-mmio modern (qemu-launcher sets
                // force-legacy=off); a legacy transport is a
                // misconfiguration, named loudly instead of half-driven.
                arch::uart_puts(
                    "nxboot: legacy virtio-mmio unsupported (boot with force-legacy=off)\n",
                );
                return None;
            }
            return Self::init(base);
        }
        None
    }

    fn init(base: usize) -> Option<Self> {
        let w = |reg: usize, v: u32| arch::mmio_write32(base + reg, v);
        let r = |reg: usize| arch::mmio_read32(base + reg);

        w(REG_STATUS, 0); // reset
        w(REG_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        w(REG_DEVICE_FEATURES_SEL, 0);
        let dev_lo = u64::from(r(REG_DEVICE_FEATURES));
        w(REG_DEVICE_FEATURES_SEL, 1);
        let dev_hi = u64::from(r(REG_DEVICE_FEATURES));
        let accept = (1u64 << VIRTIO_F_VERSION_1) & ((dev_hi << 32) | dev_lo);
        w(REG_DRIVER_FEATURES_SEL, 0);
        w(REG_DRIVER_FEATURES, accept as u32);
        w(REG_DRIVER_FEATURES_SEL, 1);
        w(REG_DRIVER_FEATURES, (accept >> 32) as u32);
        w(REG_STATUS, r(REG_STATUS) | STATUS_FEATURES_OK);
        if r(REG_STATUS) & STATUS_FEATURES_OK == 0 {
            return None;
        }

        w(REG_QUEUE_SEL, 0);
        if (r(REG_QUEUE_NUM_MAX) as usize) < QUEUE_LEN {
            return None;
        }
        w(REG_QUEUE_NUM, QUEUE_LEN as u32);

        let q = arch::queue_page_addr();
        arch::mmio_write32(base + REG_QUEUE_DESC_LOW, q as u32);
        arch::mmio_write32(base + REG_QUEUE_DESC_HIGH, ((q as u64) >> 32) as u32);
        let avail = (q + AVAIL_OFF) as u64;
        arch::mmio_write32(base + REG_QUEUE_DRIVER_LOW, avail as u32);
        arch::mmio_write32(base + REG_QUEUE_DRIVER_HIGH, (avail >> 32) as u32);
        let used = (q + USED_OFF) as u64;
        arch::mmio_write32(base + REG_QUEUE_DEVICE_LOW, used as u32);
        arch::mmio_write32(base + REG_QUEUE_DEVICE_HIGH, (used >> 32) as u32);
        w(REG_QUEUE_READY, 1);

        w(REG_STATUS, r(REG_STATUS) | STATUS_DRIVER_OK);

        let capacity =
            u64::from(r(REG_CONFIG_CAPACITY_LOW)) | (u64::from(r(REG_CONFIG_CAPACITY_HIGH)) << 32);
        Some(Self {
            base,
            capacity_sectors: capacity,
            avail_idx: Cell::new(0),
            used_seen: Cell::new(0),
        })
    }

    /// Submits one 3-descriptor chain (hdr → data → status) and polls the
    /// used ring with a bounded spin.
    fn request(&self, write: bool, sector: u64, data_addr: usize, data_len: usize) -> bool {
        let q = arch::queue_page_addr();
        let req = arch::req_page_addr();

        // Request header (device-read) + poisoned status byte.
        let req_type = if write { VIRTIO_BLK_T_OUT } else { VIRTIO_BLK_T_IN };
        arch::write_u32(req + REQ_HDR_OFF, req_type);
        arch::write_u32(req + REQ_HDR_OFF + 4, 0);
        arch::write_u64(req + REQ_HDR_OFF + 8, sector);
        arch::write_u8(req + REQ_STATUS_OFF, 0xFF);

        // Descriptor chain 0 -> 1 -> 2.
        let desc = |i: usize, addr: u64, len: u32, flags: u16, next: u16| {
            let d = q + DESC_OFF + 16 * i;
            arch::write_u64(d, addr);
            arch::write_u32(d + 8, len);
            arch::write_u16(d + 12, flags);
            arch::write_u16(d + 14, next);
        };
        desc(0, (req + REQ_HDR_OFF) as u64, 16, DESC_F_NEXT, 1);
        let data_flags = if write { DESC_F_NEXT } else { DESC_F_NEXT | DESC_F_WRITE };
        desc(1, data_addr as u64, data_len as u32, data_flags, 2);
        desc(2, (req + REQ_STATUS_OFF) as u64, 1, DESC_F_WRITE, 0);

        // Publish head 0 in the avail ring.
        let avail_idx = self.avail_idx.get();
        arch::write_u16(q + AVAIL_OFF + 4 + 2 * (avail_idx as usize % QUEUE_LEN), 0);
        arch::fence_rw();
        arch::write_u16(q + AVAIL_OFF + 2, avail_idx.wrapping_add(1));
        self.avail_idx.set(avail_idx.wrapping_add(1));
        arch::fence_rw();
        arch::mmio_write32(self.base + REG_QUEUE_NOTIFY, 0);

        // Bounded poll on used.idx (never hang on a dead device).
        let target = self.used_seen.get().wrapping_add(1);
        let mut spins = 0u32;
        while arch::read_u16(q + USED_OFF + 2) != target {
            spins += 1;
            if spins >= POLL_SPINS {
                return false;
            }
        }
        self.used_seen.set(target);
        arch::fence_rw();
        arch::read_u8(req + REQ_STATUS_OFF) == VIRTIO_BLK_S_OK
    }

    fn rw_blocks(&self, write: bool, first_block: u64, base: usize, len: usize) -> bool {
        if len % SECTOR != 0 {
            return false;
        }
        let total_sectors = (len / SECTOR) as u64;
        if first_block.saturating_add(total_sectors) > self.capacity_sectors {
            return false;
        }
        let mut done = 0usize;
        let mut lba = first_block;
        while done < len {
            let chunk = (len - done).min(MAX_RUN_SECTORS * SECTOR);
            if !self.request(write, lba, base + done, chunk) {
                return false;
            }
            done += chunk;
            lba += (chunk / SECTOR) as u64;
        }
        true
    }
}

impl BlockDevice for VirtioDisk {
    fn block_size(&self) -> usize {
        SECTOR
    }

    fn block_count(&self) -> u64 {
        self.capacity_sectors
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.read_blocks(block_idx, buf)
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        self.write_blocks(block_idx, buf)
    }

    fn read_blocks(&self, first_block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        if self.rw_blocks(false, first_block, buf.as_mut_ptr() as usize, buf.len()) {
            Ok(())
        } else {
            Err(BlockError::IoError)
        }
    }

    fn write_blocks(&mut self, first_block: u64, buf: &[u8]) -> Result<(), BlockError> {
        if self.rw_blocks(true, first_block, buf.as_ptr() as usize, buf.len()) {
            Ok(())
        } else {
            Err(BlockError::IoError)
        }
    }

    fn sync(&mut self) -> Result<(), BlockError> {
        // FLUSH is not negotiated; the BSB actuator write completes when
        // the used ring reports it (QEMU-soft durability, matches RFC).
        Ok(())
    }
}
