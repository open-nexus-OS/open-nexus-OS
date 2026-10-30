// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: Virtio-blk MMIO backend v2 (TASK-0314) — multi-sector runs,
//! real queue depth via the host-proven request ring (`crate::ring`), IRQ
//! completion (PLIC line derived from the granted transport window, own
//! minted notify endpoint) with an honest bounded-poll fallback. Split out
//! of `lib.rs` under the structure ratchet.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (bring-up)
//! TEST_COVERAGE: ring logic host-tested in `crate::ring`; this MMIO layer
//!   is proven by the QEMU persistence/`/data` ladder + blk markers.
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use core::cell::{Cell, RefCell};
use core::mem::size_of;
use core::sync::atomic::{fence, Ordering};

use crate::inflight::{interrupt, Completion, DeviceSays, InFlight, ISR_CONFIG};
use crate::ring::{Desc, Ring, RingMem, DESC_F_WRITE};
use crate::{
    QueueSetup, VirtioBlk, VirtioError, REG_QUEUE_NUM_MAX, REG_QUEUE_SEL, REG_STATUS,
    VIRTIO_DEVICE_ID_BLK, VIRTIO_MMIO_MAGIC, VIRTIO_MMIO_VERSION_LEGACY,
    VIRTIO_MMIO_VERSION_MODERN,
};
use nexus_abi::{cap_query, vmo_create_contiguous, vmo_dma_base, CapQuery, MmioWindow};
use nexus_driverkit::Mmio;
use nexus_hal::Bus;
mod completion;

const REG_INTERRUPT_STATUS: usize = 0x060;
const REG_INTERRUPT_ACK: usize = 0x064;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_T_FLUSH: u32 = 4;
const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_F_VERSION_1: u64 = 32;
const VIRTIO_BLK_F_FLUSH: u64 = 9;

/// Raw in-memory descriptor (device-visible; `next` resolved by the ring).
#[repr(C)]
#[derive(Clone, Copy)]
struct RawDesc {
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

fn align4(x: usize) -> usize {
    (x + 3) & !3usize
}

fn emit_line(msg: &str) {
    let _ = nexus_abi::debug_println(msg);
}

fn cap_query_base_len(slot: u32) -> Result<(u64, u64), VirtioError> {
    let mut info = CapQuery::default();
    cap_query(slot, &mut info).map_err(|_| VirtioError::Unsupported)?;
    Ok((info.base, info.len))
}

/// True queue depth (descriptor count) — v1's decorative 8 becomes real
/// capacity: 64 descriptors = up to 21 three-descriptor chains.
const QUEUE_LEN: usize = 64;
/// Bounded concurrent request slots (each with its own header/status/
/// data region). Synchronous callers use one; the async surface for the
/// TASK-0315 block server can fill all of them.
const IN_FLIGHT_MAX: usize = 4;
/// Max bytes one request may carry (32 sectors = a 4 KiB nxfs logical
/// block in ONE request instead of eight, with headroom for runs).
pub const MAX_RUN_BYTES: usize = 16 * 1024;

const Q_PAGES: usize = 1;
/// 1 header/status page + IN_FLIGHT_MAX data regions.
const BUF_PAGES: usize = 1 + (IN_FLIGHT_MAX * MAX_RUN_BYTES) / 4096;
const HDR_STRIDE: usize = 32;
const STATUS_OFF: usize = 2048;
/// virtio-mmio transport window on the `virt` machine — used to derive
/// the PLIC line from the granted MMIO cap (transport index + 1), the
/// same mapping hidrawd's input devices use (slots 2/3 → IRQ 3/4).

/// Device-visible queue memory behind the pure ring (volatile writes;
/// used-side reads are untrusted device input, validated by the ring).
struct QueueMem {
    desc: *mut RawDesc,
    avail: *mut VqAvail<QUEUE_LEN>,
    used: *const VqUsed<QUEUE_LEN>,
}

impl RingMem for QueueMem {
    fn write_desc(&mut self, index: u16, desc: Desc, next: u16) {
        unsafe {
            core::ptr::write_volatile(
                self.desc.add(index as usize),
                RawDesc { addr: desc.addr, len: desc.len, flags: desc.flags, next },
            );
        }
    }
    fn publish_avail(&mut self, head: u16) {
        unsafe {
            let avail = &mut *self.avail;
            let idx = core::ptr::read_volatile(&avail.idx);
            core::ptr::write_volatile(&mut avail.ring[(idx as usize) % QUEUE_LEN], head);
            // Descriptors + ring entry must be visible before the index.
            fence(Ordering::SeqCst);
            core::ptr::write_volatile(&mut avail.idx, idx.wrapping_add(1));
        }
    }
    fn used_idx(&self) -> u16 {
        unsafe { core::ptr::read_volatile(&(*self.used).idx) }
    }
    fn used_head(&self, slot: u16) -> u32 {
        unsafe { core::ptr::read_volatile(&(*self.used).ring[(slot as usize) % QUEUE_LEN]).id }
    }
}

struct DrvState {
    ring: Ring<QUEUE_LEN>,
    mem: QueueMem,
    /// The request slots (fixed header/status/data regions each) and who owns them: a slot
    /// whose request was given up stays the device's until its late completion.
    inflight: InFlight<IN_FLIGHT_MAX>,
}

/// Virtio-blk MMIO backend v2: multi-sector runs, real queue depth via
/// the host-proven request ring, completions and failures as the
/// device's interrupts (TASK-0314, TASK-0333 6f-3).
pub struct VirtioBlkMmio {
    dev: VirtioBlk<Mmio>,
    state: RefCell<DrvState>,
    buf_va: usize,
    buf_pa: u64,
    capacity_sectors: u64,
    sector_size: u32,
    irq_num: u32,
    /// The owner's notify endpoint the device's line is bound to, before the first request.
    irq_ep: u32,
    irq_logged: Cell<bool>,
    /// The device said it failed (`DEVICE_NEEDS_RESET`): every request ends in an error.
    failed: Cell<bool>,
    requests: Cell<u64>,
}

impl VirtioBlkMmio {
    /// The device behind the capability in `mmio_cap_slot`, its line bound to `irq_ep` — the
    /// notify endpoint its OWNER provisioned (a driver names no service's slots) — before its
    /// first request: every wait, the warm-up read's included, is the device's interrupt.
    pub fn new(mmio_cap_slot: u32, irq_ep: u32) -> Result<Self, VirtioError> {
        // The window is reached through the kernel-chosen map only; its physical
        // base is nobody's business here (RFC-0098 C3: the line comes with the cap).
        let _ = cap_query_base_len(mmio_cap_slot)?;
        let window =
            MmioWindow::map(mmio_cap_slot, 0, 0x1000).map_err(|_| VirtioError::Unsupported)?;
        let bus = Mmio::new(window);
        let magic = bus.read(0x000);
        if magic != VIRTIO_MMIO_MAGIC {
            return Err(VirtioError::BadMagic);
        }
        let device_id = bus.read(0x008);
        if device_id != VIRTIO_DEVICE_ID_BLK {
            return Err(VirtioError::NotBlockDevice);
        }
        let version = bus.read(0x004);
        if version != VIRTIO_MMIO_VERSION_LEGACY && version != VIRTIO_MMIO_VERSION_MODERN {
            return Err(VirtioError::UnsupportedVersion);
        }
        if version == VIRTIO_MMIO_VERSION_MODERN {
            emit_line("virtio-blk: mmio modern");
        } else {
            emit_line("virtio-blk: mmio legacy");
        }

        let dev = VirtioBlk::new(bus);
        dev.probe()?;
        dev.reset();

        // For legacy devices, set GUEST_PAGE_SIZE early (before feature
        // negotiation) — some device models expect it before any queue op.
        use crate::REG_GUEST_PAGE_SIZE;
        dev.bus.write(REG_GUEST_PAGE_SIZE, 4096);

        let driver_features = if version == VIRTIO_MMIO_VERSION_MODERN {
            (1u64 << VIRTIO_F_VERSION_1) | (1u64 << VIRTIO_BLK_F_FLUSH)
        } else {
            0
        };
        dev.negotiate_features(driver_features)?;

        // Queue memory (one page: 64 desc + avail + used fit with room), made for
        // this device — inside its DMA reach, programmed with its bus addresses.
        let q_vmo = vmo_create_contiguous(mmio_cap_slot, Q_PAGES * 4096)
            .map_err(|_| VirtioError::Unsupported)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        let q_mem_va = nexus_abi::vm_map(q_vmo, 0, Q_PAGES * 4096, flags)
            .map_err(|_| VirtioError::Unsupported)?;
        let q_base_pa = vmo_dma_base(q_vmo, mmio_cap_slot, Q_PAGES * 4096)
            .map_err(|_| VirtioError::Unsupported)?;

        let desc_bytes = size_of::<RawDesc>() * QUEUE_LEN;
        let avail_bytes = size_of::<VqAvail<QUEUE_LEN>>();
        let used_off = align4(desc_bytes + avail_bytes);
        debug_assert!(used_off + size_of::<VqUsed<QUEUE_LEN>>() <= Q_PAGES * 4096);

        let desc_va = q_mem_va;
        let avail_va = q_mem_va + desc_bytes;
        let used_va = q_mem_va + used_off;

        // Zero the queue memory BEFORE driver_ok — the device may use it
        // immediately afterwards.
        unsafe { core::ptr::write_bytes(q_mem_va as *mut u8, 0, Q_PAGES * 4096) };

        dev.bus.write(REG_QUEUE_SEL, 0);
        let q_max = dev.bus.read(REG_QUEUE_NUM_MAX);
        if (q_max as usize) < QUEUE_LEN {
            return Err(VirtioError::Unsupported);
        }

        dev.setup_queue(
            0,
            &QueueSetup {
                size: QUEUE_LEN as u16,
                desc_paddr: q_base_pa,
                avail_paddr: q_base_pa + desc_bytes as u64,
                used_paddr: q_base_pa + used_off as u64,
            },
        )?;
        dev.driver_ok();
        dev.notify_queue(0);

        // Request buffers: header/status page + per-slot data regions.
        let buf_vmo = vmo_create_contiguous(mmio_cap_slot, BUF_PAGES * 4096)
            .map_err(|_| VirtioError::Unsupported)?;
        let buf_va = nexus_abi::vm_map(buf_vmo, 0, BUF_PAGES * 4096, flags)
            .map_err(|_| VirtioError::Unsupported)?;
        let buf_pa = vmo_dma_base(buf_vmo, mmio_cap_slot, BUF_PAGES * 4096)
            .map_err(|_| VirtioError::Unsupported)?;

        let capacity_sectors = dev.capacity_sectors();

        // IRQ completion (TASK-0314): the PLIC line travels inside the granted capability
        // (RFC-0098 C3 — init read it from the node's `interrupts`); the notify endpoint is the
        // owner's (endpoint creation is init-factory-gated, RFC-0005). Bound before the first
        // request: there is no poll fallback — a device whose line cannot be bound is refused.
        let irq_num = nexus_abi::device_irq(mmio_cap_slot);
        if irq_num == 0 {
            emit_line("virtio-blk: no irq line");
            return Err(VirtioError::Unsupported);
        }
        if nexus_abi::irq_bind(irq_num, irq_ep).is_err() {
            emit_line("virtio-blk: irq bind refused");
            return Err(VirtioError::Unsupported);
        }

        let blk = Self {
            dev,
            state: RefCell::new(DrvState {
                ring: Ring::new(),
                mem: QueueMem {
                    desc: desc_va as *mut RawDesc,
                    avail: avail_va as *mut VqAvail<QUEUE_LEN>,
                    used: used_va as *const VqUsed<QUEUE_LEN>,
                },
                inflight: InFlight::new(),
            }),
            buf_va,
            buf_pa,
            capacity_sectors,
            sector_size: 512,
            irq_num,
            irq_ep,
            irq_logged: Cell::new(false),
            failed: Cell::new(false),
            requests: Cell::new(0),
        };

        emit_line("blk: irq bound");
        // Warm-up read: catches a dead device before callers rely on it.
        let mut warmup = [0u8; 512];
        if let Err(e) = blk.read_run(0, &mut warmup) {
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

    /// Virtio requests submitted since bring-up (deterministic counter
    /// for the TASK-0314 before/after measurement).
    pub fn requests_submitted(&self) -> u64 {
        self.requests.get()
    }

    /// Reads a bounded multi-sector run (len multiple of 512, ≤ 16 KiB)
    /// in ONE virtio request.
    pub fn read_run(&self, sector: u64, buf: &mut [u8]) -> Result<(), VirtioError> {
        self.transfer(VIRTIO_BLK_T_IN, sector, None, Some(buf))
    }

    /// Writes a bounded multi-sector run in ONE virtio request.
    pub fn write_run(&self, sector: u64, buf: &[u8]) -> Result<(), VirtioError> {
        self.transfer(VIRTIO_BLK_T_OUT, sector, Some(buf), None)
    }

    pub fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), VirtioError> {
        if buf.len() < 512 {
            return Err(VirtioError::Unsupported);
        }
        self.read_run(block_idx, &mut buf[..512])
    }

    pub fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), VirtioError> {
        if buf.len() < 512 {
            return Err(VirtioError::Unsupported);
        }
        self.write_run(block_idx, &buf[..512])
    }

    pub fn sync(&mut self) -> Result<(), VirtioError> {
        self.transfer(VIRTIO_BLK_T_FLUSH, 0, None, None)
    }

    /// One synchronous request through the ring: allocate a slot, build
    /// the 2/3-descriptor chain, publish, wait (IRQ or bounded poll),
    /// check the device status byte.
    fn transfer(
        &self,
        req_type: u32,
        sector: u64,
        data_in: Option<&[u8]>,
        data_out: Option<&mut [u8]>,
    ) -> Result<(), VirtioError> {
        if self.failed.get() {
            // The device said it failed: nothing it is given comes back.
            return Err(VirtioError::Unsupported);
        }
        let is_flush = req_type == VIRTIO_BLK_T_FLUSH;
        let data_len = match (&data_in, &data_out) {
            (Some(buf), None) => buf.len(),
            (None, Some(buf)) => buf.len(),
            (None, None) if is_flush => 0,
            _ => return Err(VirtioError::Unsupported),
        };
        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        let bad_len =
            !is_flush && (data_len == 0 || data_len > MAX_RUN_BYTES || data_len % 512 != 0);
        if bad_len {
            emit_line("virtio-blk: bad run len");
            return Err(VirtioError::Unsupported);
        }
        let run_sectors = (data_len / 512) as u64;
        if !is_flush
            && (sector >= self.capacity_sectors || run_sectors > self.capacity_sectors - sector)
        {
            emit_line("virtio-blk: bad sector");
            return Err(VirtioError::Unsupported);
        }

        // Late completions first: a slot the device gave back is free again.
        self.drain()?;
        // Slot + chain setup (borrow scoped: released before waiting).
        let slot_idx = {
            let mut st = self.state.borrow_mut();
            let Some(slot_idx) = st.inflight.claim() else {
                emit_line("virtio-blk: slots exhausted");
                return Err(VirtioError::Unsupported);
            };
            let hdr_va = self.buf_va + slot_idx * HDR_STRIDE;
            let hdr_pa = self.buf_pa + (slot_idx * HDR_STRIDE) as u64;
            let status_va = self.buf_va + STATUS_OFF + slot_idx;
            let status_pa = self.buf_pa + (STATUS_OFF + slot_idx) as u64;
            let data_va = self.buf_va + 4096 + slot_idx * MAX_RUN_BYTES;
            let data_pa = self.buf_pa + (4096 + slot_idx * MAX_RUN_BYTES) as u64;

            unsafe {
                core::ptr::write_volatile(
                    hdr_va as *mut BlkReq,
                    BlkReq { req_type, reserved: 0, sector },
                );
                core::ptr::write_volatile(status_va as *mut u8, 0xff);
            }
            if let Some(src) = data_in {
                unsafe {
                    core::ptr::copy_nonoverlapping(src.as_ptr(), data_va as *mut u8, data_len);
                }
            }

            let header = Desc { addr: hdr_pa, len: size_of::<BlkReq>() as u32, flags: 0 };
            let status = Desc { addr: status_pa, len: 1, flags: DESC_F_WRITE };
            let submit_result = if is_flush {
                let chain = [header, status];
                let DrvState { ring, mem, .. } = &mut *st;
                ring.submit(mem, &chain)
            } else {
                let data = Desc {
                    addr: data_pa,
                    len: data_len as u32,
                    flags: if req_type == VIRTIO_BLK_T_IN { DESC_F_WRITE } else { 0 },
                };
                let chain = [header, data, status];
                let DrvState { ring, mem, .. } = &mut *st;
                ring.submit(mem, &chain)
            };
            let head = match submit_result {
                Ok(head) => head,
                Err(_) => {
                    emit_line("virtio-blk: ring exhausted");
                    return Err(VirtioError::Unsupported);
                }
            };
            let _ = st.inflight.submitted(slot_idx, head);
            slot_idx
        };
        self.requests.set(self.requests.get().wrapping_add(1));

        fence(Ordering::SeqCst);
        self.dev.notify_queue(0);

        if let Err(e) = self.wait_done(slot_idx) {
            // The wait failed: the device keeps the chain, and a late completion frees the
            // slot; no interrupt is left claimed for the next request to miss.
            self.state.borrow_mut().inflight.abandon(slot_idx);
            let _ = self.ack_irq();
            return Err(e);
        }

        if let Some(out) = data_out {
            let data_va = self.buf_va + 4096 + slot_idx * MAX_RUN_BYTES;
            unsafe {
                core::ptr::copy_nonoverlapping(data_va as *const u8, out.as_mut_ptr(), data_len);
            }
        }
        let status_va = self.buf_va + STATUS_OFF + slot_idx;
        let status = unsafe { core::ptr::read_volatile(status_va as *const u8) };
        let _ = self.state.borrow_mut().inflight.release(slot_idx);
        if status != VIRTIO_BLK_S_OK {
            emit_line("virtio-blk: status err");
            return Err(VirtioError::Unsupported);
        }
        Ok(())
    }

    /// Drains completed chains from the used ring into the in-flight table: a waiter's request
    /// is done, a request given up on frees its slot. Device-provided ids are validated by the
    /// ring.
    fn drain(&self) -> Result<(), VirtioError> {
        let mut st = self.state.borrow_mut();
        loop {
            let DrvState { ring, mem, inflight } = &mut *st;
            match ring.complete(mem) {
                Ok(Some(head)) => {
                    if let Completion::Reclaimed(_) = inflight.completed(head) {
                        emit_line("virtio-blk: late completion (slot back)");
                    }
                }
                Ok(None) => return Ok(()),
                Err(_) => {
                    emit_line("virtio-blk: bad used id");
                    return Err(VirtioError::Unsupported);
                }
            }
        }
    }

    /// Drain queued notifications, clear InterruptStatus, THEN
    /// `irq_complete` — the virtio-input/gpud ordering lesson: any other
    /// order lets the source storm or lose the edge. Answers what the
    /// interrupt said (the device status read on a configuration change).
    fn ack_irq(&self) -> DeviceSays {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 16];
        for _ in 0..8 {
            if nexus_abi::ipc_recv_v1_nb(self.irq_ep, &mut hdr, &mut buf, true).is_err() {
                break;
            }
        }
        let isr = self.dev.bus.read(REG_INTERRUPT_STATUS);
        let status = if isr & ISR_CONFIG != 0 { self.dev.bus.read(REG_STATUS) } else { 0 };
        if isr != 0 {
            self.dev.bus.write(REG_INTERRUPT_ACK, isr);
        }
        let _ = nexus_abi::irq_complete(self.irq_num);
        interrupt(isr, status)
    }
}
