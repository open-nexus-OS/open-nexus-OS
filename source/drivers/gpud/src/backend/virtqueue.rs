// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The virtio control/cursor virtqueue with its multi-entry command ring.
//!
//! Owns the virtqueue memory layout (`VqDesc`/`VqAvail`/`VqUsed`), the ring
//! sizing constants, and `CtrlQueue` — the per-slot lifecycle ring (enqueue →
//! notify → harvest) built on the shared DriverKit `SubmitRing`. Completion is
//! pure-reactive: harvest the used-ring once, otherwise block on the GPU
//! ring-buffer IRQ, never a busy spin. The `GPU_WAIT_DEADLINE_NS` safety net
//! only bounds a lost/late IRQ so a present can never hang.

#![cfg(all(feature = "os-lite", target_os = "none"))]

use super::transport::{align4, read_reg, write_reg, write_u64_pair};
use crate::error::GpuDriverError;
use crate::protocol;
use nexus_gfx::backend::error::GfxError;

pub(crate) const CTRL_QUEUE_INDEX: u32 = 0;
#[allow(dead_code)]
pub(crate) const CURSOR_QUEUE_INDEX: u32 = 1;
/// Maximum in-flight commands on the control queue = command slots in the ring.
/// A present batches ~8 SUBMIT_3D draws + a flush; 16 gives headroom so a whole
/// present is enqueued without an intra-batch drain. The cursor queue passes
/// `slots = 1` (single-slot, unchanged behaviour).
pub(crate) const RING_SLOTS: usize = 16;
/// virtqueue descriptor-table length. Each command slot uses a 2-descriptor chain
/// (cmd → resp), so the table holds `RING_SLOTS * 2` descriptors. `avail.ring` /
/// `used.ring` are sized to this; both queues share the length (the cursor queue
/// uses only the first pair).
pub(crate) const QUEUE_LEN: usize = RING_SLOTS * 2;
/// Hard ceiling on any single GPU command wait. The used-ring advance normally
/// completes far sooner (spin or IRQ); this only bounds a lost/late IRQ so a
/// present can never hang — it degrades to the legacy timeout (matches the old
/// 500 ms spin deadline).
pub(crate) const GPU_WAIT_DEADLINE_NS: u64 = 500_000_000;
// Completion is PURE REACTIVE: `wait_slot`/`alloc_free_slot` `harvest` the used-ring
// once at the top of the loop (an already-finished command returns immediately, no
// syscall), and otherwise BLOCK on the GPU ring-buffer IRQ via `block_on_irq` — never
// a busy yield-spin (a spin IS a poll, which we explicitly do not want). The pipelined
// present blocks on nothing at all; the next frame harvests. `GPU_WAIT_DEADLINE_NS` is
// only the safety net bounding a lost/late IRQ so a present can never hang.
/// Latches once the GPU ring-buffer IRQ first wakes a completion wait, so the
/// headless run can confirm the interrupt path is actually live (vs. silently
/// degrading to the spin fallback). One marker, not per-frame — no UART storm.
pub(crate) static GPU_IRQ_WAKE_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
/// Latches once `harvest` first reclaims a completed slot — proof (once) that the
/// pipelined completion path flows (frame N's commands complete and are observed
/// asynchronously, without the present ever blocking on them). One marker, not
/// per-frame.
pub(crate) static PIPELINE_HARVEST_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

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
}

/// A virtio control/cursor virtqueue with a multi-entry command ring.
///
/// Holds raw pointers into device-shared memory (descriptor table, avail/used
/// rings, command/response pools), so it is intentionally **not `Send`/`Sync`**:
/// the buffers live in gpud's address space and the ring is driven by gpud's
/// single cooperative thread (enqueue → notify → drain is one logical sequence).
/// There is no `unsafe impl Send` — the queue must never cross threads, and the
/// `!Send` default enforces that at compile time.
pub(crate) struct CtrlQueue {
    queue_index: u32,
    _queue_vmo: u32,
    _cmd_vmo: u32,
    _resp_vmo: u32,
    desc: *mut VqDesc,
    avail: *mut VqAvail<QUEUE_LEN>,
    used: *mut VqUsed<QUEUE_LEN>,
    /// Command-buffer pool base (VA/PA). Slot `i`'s command buffer is at
    /// `cmd_va + i*4096` / `cmd_pa + i*4096` (the pool is physically contiguous).
    cmd_va: usize,
    cmd_pa: u64,
    /// Response-buffer pool base (VA/PA). Slot `i`'s response header is at
    /// `resp_va + i*RESP_SLOT_SIZE` / `resp_pa + i*RESP_SLOT_SIZE`.
    resp_va: usize,
    resp_pa: u64,
    /// Slot lifecycle — in-flight set, round-robin allocation, backpressure — provided by
    /// the shared DriverKit submit ring (RFC-0033 `nexus_driverkit::SubmitRing`, the lib that
    /// generalises this very ring). A slot is reserved on `try_alloc` and freed only when its
    /// used-ring entry is harvested (`complete`), so it is never reused while QEMU may still
    /// be reading its buffers — the pipelining safety invariant. The virtio specifics
    /// (descriptor pairs, cmd/resp pools, the `last_used` cursor) stay here in gpud.
    pub(crate) ring: nexus_driverkit::SubmitRing,
    /// Device `used.idx` already harvested — the consumer cursor into `used.ring`.
    last_used: u16,
    /// Device MMIO base — needed to drain/ACK InterruptStatus (0x60/0x64) on the
    /// GPU ring-buffer IRQ path so the level-triggered line de-asserts.
    mmio_base: usize,
    /// PLIC source bound to this queue's completion IRQ (0 = not bound → the
    /// legacy spin+yield wait is used, never a hang).
    irq_num: u32,
    /// Endpoint cap slot the kernel routes the GPU IRQ to (0 = not bound). When
    /// set, the wait path blocks here instead of busy-polling.
    irq_ep: u32,
}

/// Bytes reserved per response sub-slot in the response pool (a virtio-gpu
/// response header is 24 B; 256 B keeps slots cache-line-friendly and lets the
/// whole `RING_SLOTS` pool fit one 4 KiB page).
pub(crate) const RESP_SLOT_SIZE: usize = 256;

/// A command slot in the multi-entry ring (`0..slots`). A newtype so it can't be
/// confused with a raw descriptor index (each slot owns the descriptor *pair*
/// `2*slot` / `2*slot+1`) or with an in-flight *count* — the three are different
/// quantities that all happen to be small integers, and mixing them in the
/// pointer/descriptor arithmetic would be a silent memory-safety bug.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct RingSlot(u16);

impl RingSlot {
    /// Head (command) descriptor index for this slot's 2-descriptor chain.
    #[inline]
    fn head_desc(self) -> usize {
        2 * self.0 as usize
    }
    /// Response descriptor index (`head + 1`).
    #[inline]
    fn resp_desc(self) -> usize {
        2 * self.0 as usize + 1
    }
}

impl CtrlQueue {
    /// `slots` = number of in-flight command buffers (control = `RING_SLOTS`,
    /// cursor = 1). The command pool is `slots` contiguous 4 KiB pages; the
    /// response pool is one page (`slots` × `RESP_SLOT_SIZE`). `slots` must be
    /// ≤ `RING_SLOTS` so the descriptor table (`QUEUE_LEN`) and the single
    /// response page suffice.
    pub(crate) fn new(
        mmio_base: usize,
        queue_index: u32,
        queue_va: usize,
        cmd_va_base: usize,
        resp_va_base: usize,
        slots: usize,
    ) -> Result<Self, GpuDriverError> {
        debug_assert!(slots >= 1 && slots <= RING_SLOTS);
        debug_assert!(slots * RESP_SLOT_SIZE <= 4096);
        let cmd_pool_len = slots * 4096;
        let q_vmo = nexus_abi::vmo_create(4096).map_err(|_| GpuDriverError::MmioFault)?;
        let cmd_vmo = nexus_abi::vmo_create(cmd_pool_len).map_err(|_| GpuDriverError::MmioFault)?;
        let resp_vmo = nexus_abi::vmo_create(4096).map_err(|_| GpuDriverError::MmioFault)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        nexus_abi::vmo_map_page(q_vmo, queue_va, 0, flags)
            .map_err(|_| GpuDriverError::MmioFault)?;
        // Map the whole command pool (one page per in-flight slot, contiguous).
        for i in 0..slots {
            nexus_abi::vmo_map_page(cmd_vmo, cmd_va_base + i * 4096, i * 4096, flags)
                .map_err(|_| GpuDriverError::MmioFault)?;
        }
        nexus_abi::vmo_map_page(resp_vmo, resp_va_base, 0, flags)
            .map_err(|_| GpuDriverError::MmioFault)?;
        let mut q_info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        let mut cmd_info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        let mut resp_info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        nexus_abi::cap_query(q_vmo, &mut q_info).map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::cap_query(cmd_vmo, &mut cmd_info).map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::cap_query(resp_vmo, &mut resp_info).map_err(|_| GpuDriverError::MmioFault)?;
        unsafe {
            core::ptr::write_bytes(queue_va as *mut u8, 0, 4096);
            core::ptr::write_bytes(cmd_va_base as *mut u8, 0, cmd_pool_len);
            core::ptr::write_bytes(resp_va_base as *mut u8, 0, 4096);
        }

        let desc_bytes = core::mem::size_of::<VqDesc>() * QUEUE_LEN;
        let avail_bytes = core::mem::size_of::<VqAvail<QUEUE_LEN>>();
        let used_off = align4(desc_bytes + avail_bytes);
        let desc_va = queue_va;
        let avail_va = queue_va + desc_bytes;
        let used_va = queue_va + used_off;

        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_SEL, queue_index);
        let max = read_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_NUM_MAX);
        if max < QUEUE_LEN as u32 {
            return Err(GpuDriverError::ResourceExhausted);
        }
        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_NUM, QUEUE_LEN as u32);
        write_u64_pair(mmio_base, protocol::VIRTIO_MMIO_QUEUE_DESC_LOW, q_info.base);
        write_u64_pair(
            mmio_base,
            protocol::VIRTIO_MMIO_QUEUE_DRIVER_LOW,
            q_info.base + desc_bytes as u64,
        );
        write_u64_pair(
            mmio_base,
            protocol::VIRTIO_MMIO_QUEUE_DEVICE_LOW,
            q_info.base + used_off as u64,
        );
        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_READY, 1);

        Ok(Self {
            queue_index,
            _queue_vmo: q_vmo,
            _cmd_vmo: cmd_vmo,
            _resp_vmo: resp_vmo,
            desc: desc_va as *mut VqDesc,
            avail: avail_va as *mut VqAvail<QUEUE_LEN>,
            used: used_va as *mut VqUsed<QUEUE_LEN>,
            cmd_va: cmd_va_base,
            cmd_pa: cmd_info.base,
            resp_va: resp_va_base,
            resp_pa: resp_info.base,
            ring: nexus_driverkit::SubmitRing::new(slots),
            last_used: 0,
            mmio_base,
            irq_num: 0,
            irq_ep: 0,
        })
    }

    pub(crate) fn submit(&mut self, mmio_base: usize, bytes: &[u8]) -> Result<(), GfxError> {
        self.submit_two(mmio_base, bytes, &[])
    }

    /// Bind this queue to a GPU ring-buffer IRQ so the completion wait can BLOCK
    /// on the interrupt instead of busy-polling. `irq_ep` is the endpoint cap slot
    /// the kernel routes the PLIC source to (set via `irq_bind`); `irq_num` is that
    /// source. Both 0 keeps the legacy spin+yield path.
    pub(crate) fn set_gpu_irq(&mut self, irq_num: u32, irq_ep: u32) {
        self.irq_num = irq_num;
        self.irq_ep = irq_ep;
    }

    // ── slot addressing (the command/response buffer pools are contiguous) ──
    #[inline]
    fn cmd_slot_va(&self, slot: RingSlot) -> usize {
        self.cmd_va + slot.0 as usize * 4096
    }
    #[inline]
    fn cmd_slot_pa(&self, slot: RingSlot) -> u64 {
        self.cmd_pa + slot.0 as u64 * 4096
    }
    #[inline]
    fn resp_slot_va(&self, slot: RingSlot) -> usize {
        self.resp_va + slot.0 as usize * RESP_SLOT_SIZE
    }
    #[inline]
    fn resp_slot_pa(&self, slot: RingSlot) -> u64 {
        self.resp_pa + slot.0 as u64 * RESP_SLOT_SIZE as u64
    }

    /// Reap completed commands: walk the new `used.ring` entries and free their
    /// slots. The used element's `id` is the head descriptor (`2*slot`), so `id/2`
    /// maps a completion back to its slot. This is the consumer half of the
    /// pipeline — frame N's completion is observed here (typically during frame
    /// N+1's enqueue), so the present never blocks on its own completion.
    pub(crate) fn harvest(&mut self) {
        let used_idx = unsafe { core::ptr::read_volatile(&(*self.used).idx) };
        while self.last_used != used_idx {
            let elem = unsafe {
                core::ptr::read_volatile(&(*self.used).ring[self.last_used as usize % QUEUE_LEN])
            };
            let slot = (elem.id / 2) as usize;
            if slot < self.ring.capacity() {
                // Free the slot. Idempotent: a spurious/duplicate completion for an
                // already-free slot is ignored (`complete` errors, no double-count) — same
                // as the old `busy &= !(1<<slot)` bitmask clear.
                let _ = self.ring.complete(nexus_driverkit::Slot(slot as u8));
            }
            self.last_used = self.last_used.wrapping_add(1);
        }
    }

    /// Round-robin reservation of a free slot (harvest first). `None` = ring full.
    fn find_free_slot(&mut self) -> Option<RingSlot> {
        self.harvest();
        // Reserve via the shared ring. Reserving here rather than at `publish` is
        // behaviour-equivalent: every `find_free_slot` / `alloc_free_slot` is unconditionally
        // followed by `publish` (no early return between), so a reserved slot is always
        // submitted — no leak. `RING_SLOTS ≤ 16` so the u8→u16 widen is lossless.
        self.ring.try_alloc().map(|(slot, _ticket)| RingSlot(slot.0 as u16))
    }

    /// Allocate a free slot, applying back-pressure if the ring is full: block on
    /// the GPU IRQ + harvest until one frees (deadline-bounded). On a (degraded)
    /// timeout, force-resync the in-flight set so the ring can never deadlock.
    fn alloc_free_slot(&mut self) -> Result<RingSlot, GfxError> {
        if let Some(slot) = self.find_free_slot() {
            return Ok(slot);
        }
        let start = nexus_abi::nsec().map_err(|_| GfxError::MmioFault)?;
        let deadline = start.saturating_add(GPU_WAIT_DEADLINE_NS);
        loop {
            self.block_on_irq(deadline);
            if let Some(slot) = self.find_free_slot() {
                if self.irq_ep != 0 {
                    self.ack_gpu_irq();
                }
                return Ok(slot);
            }
            if nexus_abi::nsec().map_err(|_| GfxError::MmioFault)? >= deadline {
                // Degraded recovery: abandon the stuck in-flight set + resync the
                // harvest cursor so we never wedge. Best-effort (a lost IRQ only).
                self.ring.reset();
                self.last_used = unsafe { core::ptr::read_volatile(&(*self.used).idx) };
                if self.irq_ep != 0 {
                    self.ack_gpu_irq();
                }
                // `reset` emptied the ring, so this reservation always succeeds.
                return self
                    .ring
                    .try_alloc()
                    .map(|(slot, _)| RingSlot(slot.0 as u16))
                    .ok_or(GfxError::MmioFault);
            }
        }
    }

    /// Block once on the GPU ring-buffer IRQ (deadline-bounded) or yield if the
    /// queue isn't IRQ-bound. The reactive wait primitive shared by `wait_slot`
    /// and `alloc_free_slot`.
    fn block_on_irq(&self, deadline: u64) {
        if self.irq_ep != 0 {
            let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 16];
            if nexus_abi::ipc_recv_v1(
                self.irq_ep,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_TRUNCATE,
                deadline,
            )
            .is_ok()
                && !GPU_IRQ_WAKE_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed)
            {
                // Proof (once): a real GPU ring-buffer IRQ woke a wait.
                let _ = nexus_abi::debug_println("gpud: gpu irq wake");
            }
        } else {
            let _ = nexus_abi::yield_();
        }
    }

    /// Make a written descriptor chain available to the device + notify, and mark
    /// the slot in-flight (freed later by `harvest` when its completion returns).
    #[inline]
    fn publish(&mut self, mmio_base: usize, slot: RingSlot) {
        let head = slot.head_desc();
        unsafe {
            let idx = core::ptr::read_volatile(&(*self.avail).idx);
            core::ptr::write_volatile(
                &mut (*self.avail).ring[(idx as usize) % QUEUE_LEN],
                head as u16,
            );
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            core::ptr::write_volatile(&mut (*self.avail).idx, idx.wrapping_add(1));
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_NOTIFY, self.queue_index);
        // The slot was already reserved (marked in-flight) at alloc by `ring.try_alloc`;
        // `harvest` frees it when the completion returns. (Was `self.busy |= 1<<slot` here.)
    }

    /// Enqueue a command that expects a device response (cmd → resp 2-descriptor
    /// chain) WITHOUT waiting for completion. The caller `drain`s the batch later
    /// and may inspect the response at the returned slot. `first`+`second` are
    /// concatenated into the slot's command buffer (`second` empty = single blob).
    pub(crate) fn enqueue_pair(
        &mut self,
        mmio_base: usize,
        first: &[u8],
        second: &[u8],
    ) -> Result<RingSlot, GfxError> {
        let total = first.len().checked_add(second.len()).ok_or(GfxError::ResourceExhausted)?;
        if total == 0 || total > 4096 || core::mem::size_of::<protocol::VirtioGpuCtrlHdr>() > total
        {
            return Err(GfxError::CommandRejected);
        }
        let slot = self.alloc_free_slot()?;
        let head = slot.head_desc();
        let cmd_va = self.cmd_slot_va(slot);
        let cmd_pa = self.cmd_slot_pa(slot);
        let resp_pa = self.resp_slot_pa(slot);
        unsafe {
            core::ptr::write_bytes(cmd_va as *mut u8, 0, total);
            core::ptr::write_bytes(self.resp_slot_va(slot) as *mut u8, 0, RESP_SLOT_SIZE);
            core::ptr::copy_nonoverlapping(first.as_ptr(), cmd_va as *mut u8, first.len());
            if !second.is_empty() {
                core::ptr::copy_nonoverlapping(
                    second.as_ptr(),
                    (cmd_va + first.len()) as *mut u8,
                    second.len(),
                );
            }
            core::ptr::write_volatile(
                self.desc.add(head),
                VqDesc { addr: cmd_pa, len: total as u32, flags: 1, next: slot.resp_desc() as u16 },
            );
            core::ptr::write_volatile(
                self.desc.add(slot.resp_desc()),
                VqDesc {
                    addr: resp_pa,
                    len: core::mem::size_of::<protocol::VirtioGpuCtrlHdr>() as u32,
                    flags: 2,
                    next: 0,
                },
            );
        }
        self.publish(mmio_base, slot);
        Ok(slot)
    }

    /// Enqueue a response-less command (single read-only descriptor) WITHOUT
    /// waiting. Used by the cursor queue (UPDATE/MOVE_CURSOR carry no response).
    fn enqueue_single(&mut self, mmio_base: usize, bytes: &[u8]) -> Result<RingSlot, GfxError> {
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err(GfxError::CommandRejected);
        }
        let slot = self.alloc_free_slot()?;
        let head = slot.head_desc();
        let cmd_va = self.cmd_slot_va(slot);
        let cmd_pa = self.cmd_slot_pa(slot);
        unsafe {
            core::ptr::write_bytes(cmd_va as *mut u8, 0, bytes.len());
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), cmd_va as *mut u8, bytes.len());
            core::ptr::write_volatile(
                self.desc.add(head),
                VqDesc { addr: cmd_pa, len: bytes.len() as u32, flags: 0, next: 0 },
            );
        }
        self.publish(mmio_base, slot);
        Ok(slot)
    }

    /// Synchronously wait for ONE slot's completion. Used by `submit_two` /
    /// `submit_no_response` for the init + 2D/mmio paths, where each command's
    /// response must be in hand before the next is issued. Harvest-driven +
    /// reactive (block on the GPU ring-buffer IRQ), bounded by `GPU_WAIT_DEADLINE_NS`
    /// so a lost/late IRQ degrades to a timeout, never a hang.
    ///
    /// The pipelined present does NOT call this — it enqueues and lets the next
    /// frame `harvest` the completion (so a deferred textured-draw completion never
    /// blocks the present).
    fn wait_slot(&mut self, slot: RingSlot) -> Result<(), GfxError> {
        let dk_slot = nexus_driverkit::Slot(slot.0 as u8);
        let start = nexus_abi::nsec().map_err(|_| GfxError::MmioFault)?;
        let deadline = start.saturating_add(GPU_WAIT_DEADLINE_NS);
        loop {
            self.harvest();
            if !self.ring.is_in_flight(dk_slot) {
                if self.irq_ep != 0 {
                    self.ack_gpu_irq();
                }
                return Ok(());
            }
            if nexus_abi::nsec().map_err(|_| GfxError::MmioFault)? >= deadline {
                // Abandon the stuck slot (degraded, lost-IRQ only): free it WITHOUT counting
                // a completion (the command never finished), so a fence can't jump past it.
                self.ring.abandon(dk_slot);
                if self.irq_ep != 0 {
                    self.ack_gpu_irq();
                }
                return Err(GfxError::MmioFault);
            }
            self.block_on_irq(deadline);
        }
    }

    /// De-assert + re-arm this queue's GPU IRQ. Order matters (same lesson as
    /// virtio-input): drain the queued notification, clear the device's
    /// InterruptStatus, THEN `irq_complete` so the source can't immediately storm.
    fn ack_gpu_irq(&self) {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 16];
        let _ = nexus_abi::ipc_recv_v1_nb(self.irq_ep, &mut hdr, &mut buf, true);
        let status = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_INTERRUPT_STATUS);
        if status != 0 {
            write_reg(self.mmio_base, protocol::VIRTIO_MMIO_INTERRUPT_ACK, status);
        }
        let _ = nexus_abi::irq_complete(self.irq_num);
    }

    /// Submit a command that the device completes WITHOUT writing a response
    /// payload. The virtio-gpu cursor queue is such a queue: QEMU processes
    /// UPDATE_CURSOR/MOVE_CURSOR and pushes the used element with len=0 and no
    /// response header. Posting a response descriptor and demanding
    /// RESP_OK_NODATA (like `submit`) therefore always "fails" — the historical
    /// reason the hardware cursor was abandoned. Single read-only descriptor,
    /// completion = used-ring advance.
    pub(crate) fn submit_no_response(&mut self, mmio_base: usize, bytes: &[u8]) -> Result<(), GfxError> {
        // Single read-only command, no response payload to inspect — the used-ring
        // advance IS the completion. Enqueue then wait for that slot (synchronous).
        let slot = self.enqueue_single(mmio_base, bytes)?;
        self.wait_slot(slot)
    }

    /// Synchronous single command: enqueue one cmd→resp pair, wait for that slot,
    /// classify the response. Behaviour is identical to the pre-pipeline path —
    /// used by init, the 2D/mmio present, and every non-batched caller. The
    /// pipelined present instead `enqueue_pair`s every draw and never waits (the
    /// next frame `harvest`s the completion).
    pub(crate) fn submit_two(
        &mut self,
        mmio_base: usize,
        first: &[u8],
        second: &[u8],
    ) -> Result<(), GfxError> {
        let slot = self.enqueue_pair(mmio_base, first, second)?;
        self.wait_slot(slot)?;
        self.classify_resp(slot, "ctrl")
    }

    /// Classify a drained slot's device response. RESP_OK_NODATA → Ok; any error
    /// type is logged (the string names the exact QEMU rejection) → CommandRejected.
    fn classify_resp(&self, slot: RingSlot, label: &str) -> Result<(), GfxError> {
        let hdr = unsafe {
            core::ptr::read_volatile(self.resp_slot_va(slot) as *const protocol::VirtioGpuCtrlHdr)
        };
        if hdr.type_ == protocol::VIRTIO_GPU_RESP_OK_NODATA {
            return Ok(());
        }
        // Debug: classify the error response from QEMU
        match hdr.type_ {
            0x1200 => {
                let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_UNSPEC");
                // Log which command was rejected
                let _ = nexus_abi::debug_println(label);
            }
            0x1201 => {
                let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_OUT_OF_MEMORY");
            }
            0x1202 => {
                let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_INVALID_SCANOUT_ID");
            }
            0x1203 => {
                let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_INVALID_RESOURCE_ID");
            }
            0x1204 => {
                let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_INVALID_CONTEXT_ID");
            }
            0x1205 => {
                let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_INVALID_PARAMETER");
            }
            _ => {
                let _ = nexus_abi::debug_println("gpud: dbg resp=UNKNOWN");
            }
        }
        Err(GfxError::CommandRejected)
    }
}
