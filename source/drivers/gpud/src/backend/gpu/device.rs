// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The device online (TASK-0329 G1 P3/P4): the bring-up ladder — glue through socd, the window
//! and the core id, the firmware placed and started, the first interrupt and `firmware_started`,
//! a health-check round trip through the kernel command buffer, one GPU address space with a
//! page bound, flushed, unbound — each rung a line a board cycle is judged from, each FAIL
//! form with the registers that tell our fault from the firmware's. The interrupt is waited
//! for on gpud's own notify endpoint (bounded, in slices: the firmware's boot raises no
//! interrupt the interface promises), never spun for. The kernel command buffer's send is one
//! command written, cleaned, published, kicked.

use nexus_abi::Cap;
use nexus_gfx::backend::gpu::ccb::{Fwccb, FwccbMemory, Kccb, KccbMemory};
use nexus_gfx::backend::gpu::fwif::{self, CcbCtl, FwMemContext, KccbCmd};
use nexus_gfx::backend::gpu::mmu::{PageFlags, PageTables, TablePage, TablePages};
use nexus_gfx::backend::gpu::start;
use nexus_gfx::backend::gpu::{heaps, CoreId};
use nexus_ipc::timer::{NotifyTimer, Waitset};
use nexus_service_topology::DEVICE_MMIO_SLOT;

use super::firmware::{self, Loaded};
use super::memory::{FwMemory, HeapObj};
use super::regs::Window;
use super::{emit, glue, job};

/// The firmware's boot budget (the open driver's).
const FW_BOOT_TIMEOUT_NS: u64 = 5_000_000_000;
/// A kernel command's budget.
const KCCB_TIMEOUT_NS: u64 = 1_000_000_000;
/// One wait slice: the interrupt wakes the wait early; the slice bounds a boot that raises
/// none.
const WAIT_SLICE_NS: u64 = 1_000_000;

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

pub(crate) struct Gpu {
    pub(super) win: Window,
    pub(super) mem: FwMemory,
    pub(super) fw: Loaded,
    pub(super) kccb: Kccb,
    pub(super) irq: u32,
    pub(super) irq_slot: u32,
    /// The bound of every wait: gpud's declared watchdog one-shot, a waitset member beside the
    /// interrupt endpoint (never a deadline on the receive itself).
    timer: NotifyTimer,
    waits: Waitset,
    /// Interrupts served since bind.
    pub(super) irqs: u32,
    /// The kernel timeline fence the null job's completions are signalled on.
    pub(super) fence: Cap,
    /// The bring-up's own address space and its firmware memory context.
    pub(super) tables: Option<PageTables>,
    pub(super) fw_mem_ctx: Option<HeapObj>,
    pub(super) context: Option<job::Context>,
}

/// The kernel command buffer's memory, over the firmware heap.
struct KccbMem<'a> {
    mem: &'a mut FwMemory,
    ctl: HeapObj,
    slots: HeapObj,
    rtn: HeapObj,
}

impl KccbMemory for KccbMem<'_> {
    fn read_offset(&self) -> u32 {
        self.mem.read_u32(&self.ctl, CcbCtl::READ_OFFSET as u32)
    }
    fn set_write_offset(&mut self, offset: u32) {
        self.mem.write_u32(&self.ctl, CcbCtl::WRITE_OFFSET as u32, offset);
    }
    fn write_slot(&mut self, slot: u32, cmd: &[u8; KccbCmd::SIZE]) {
        self.mem.write(&self.slots, slot * KccbCmd::SIZE as u32, cmd);
    }
    fn rtn(&self, slot: u32) -> u32 {
        self.mem.read_u32(&self.rtn, slot * 4)
    }
    fn set_rtn(&mut self, slot: u32, value: u32) {
        self.mem.write_u32(&self.rtn, slot * 4, value);
    }
}

struct FwccbMem<'a> {
    mem: &'a mut FwMemory,
    ctl: HeapObj,
    slots: HeapObj,
}

impl FwccbMemory for FwccbMem<'_> {
    fn write_offset(&self) -> u32 {
        self.mem.read_u32(&self.ctl, CcbCtl::WRITE_OFFSET as u32)
    }
    fn read_offset(&self) -> u32 {
        self.mem.read_u32(&self.ctl, CcbCtl::READ_OFFSET as u32)
    }
    fn set_read_offset(&mut self, offset: u32) {
        self.mem.write_u32(&self.ctl, CcbCtl::READ_OFFSET as u32, offset);
    }
    fn read_slot(&self, slot: u32) -> [u8; fwif::FwccbCmd::SIZE] {
        let mut b = [0u8; fwif::FwccbCmd::SIZE];
        self.mem.read(&self.slots, slot * fwif::FwccbCmd::SIZE as u32, &mut b);
        b
    }
}

/// The GPU MMU's table pages, from the firmware heap's backing pool.
pub(super) struct Tables<'a>(pub(super) &'a mut FwMemory);

impl TablePages for Tables<'_> {
    fn alloc(&mut self) -> Option<TablePage> {
        self.0.alloc_page().map(|p| TablePage { handle: p.index as u32, phys: p.phys })
    }
    fn free(&mut self, _page: TablePage) {
        // The pool is a bump allocator for the bring-up's life (TASK-0329 G1); layer 4's
        // device returns pages.
    }
    fn write_u32(&mut self, page: TablePage, index: usize, value: u32) {
        let p = super::memory::Page { index: page.handle as usize, phys: page.phys };
        self.0.page_write_u32(&p, index, value);
    }
    fn write_u64(&mut self, page: TablePage, index: usize, value: u64) {
        let p = super::memory::Page { index: page.handle as usize, phys: page.phys };
        self.0.page_write_u64(&p, index, value);
    }
}

impl Gpu {
    /// The ladder; `None` after a FAIL line (the display keeps running without the GPU). The
    /// state lives on the heap and the ladder is never inlined into the service loop: gpud's
    /// stack is 32 KiB and the loop's frame is static (cycle 2026-10-13: the QEMU lane's gpud
    /// overflowed its stack with the GPU state in the frame — on a machine without the GPU).
    #[inline(never)]
    pub(crate) fn bring_up(irq_slot: u32) -> Option<alloc::boxed::Box<Gpu>> {
        glue::bring_up()?;
        let mut win = match Window::map() {
            Ok(w) => w,
            Err(e) => {
                emit(&alloc::format!("gpud: FAIL gpu window ({e})"));
                return None;
            }
        };
        let id = CoreId(win.core_id());
        if id != CoreId::BOARD {
            emit(&alloc::format!(
                "gpud: FAIL gpu core (id={:#018x}: {})",
                id.0,
                if id.0 == u64::MAX {
                    "the window floats — domain, clock or reset"
                } else {
                    "another core"
                }
            ));
            return None;
        }
        emit(&alloc::format!(
            "gpud: gpu core ok (bvnc={}.{}.{}.{} id={:#018x})",
            id.b(),
            id.v(),
            id.n(),
            id.c(),
            id.0
        ));

        let mut info = nexus_abi::CapQuery::default();
        if nexus_abi::cap_query(DEVICE_MMIO_SLOT, &mut info).is_err() {
            emit("gpud: FAIL gpu window (no capability)");
            return None;
        }
        let block = match nexus_abi::device_dma_coherence(DEVICE_MMIO_SLOT) {
            Ok(nexus_abi::DmaCoherence::Coherent) => 0,
            Ok(nexus_abi::DmaCoherence::Maintained { block }) => block,
            _ => {
                emit("gpud: FAIL gpu memory (non-coherent device without cache maintenance)");
                return None;
            }
        };
        let mut mem = match FwMemory::new(DEVICE_MMIO_SLOT, firmware::BACKING_PAGES, block) {
            Ok(m) => m,
            Err(e) => {
                emit(&alloc::format!("gpud: FAIL gpu memory ({e})"));
                return None;
            }
        };
        let fw = match firmware::load(&mut mem, info.base, 614_400_000) {
            Ok(l) => l,
            Err(e) => {
                emit(&alloc::format!("gpud: FAIL gpu firmware ({})", e.0));
                return None;
            }
        };
        // The interrupt first: a boot's first wake is not missed.
        let irq = info.irq;
        if nexus_abi::irq_bind(irq, irq_slot).is_err() {
            emit(&alloc::format!("gpud: FAIL gpu irq (line {irq} not bound)"));
            return None;
        }
        let Ok(fence) = nexus_abi::fence_create() else {
            emit("gpud: FAIL gpu fence (no kernel fence)");
            return None;
        };
        let Ok(timer) = NotifyTimer::bind(nexus_service_topology::slots::gpud::WATCHDOG) else {
            emit("gpud: FAIL gpu wait (no watchdog timer pair)");
            return None;
        };
        let Ok(waits) = Waitset::over(&[irq_slot, timer.recv_slot()]) else {
            emit("gpud: FAIL gpu wait (no waitset)");
            return None;
        };
        let t0 = now_ns();
        start::start(&mut win, &fw.boot, &fw.features);
        let mut gpu = alloc::boxed::Box::new(Gpu {
            win,
            mem,
            fw,
            kccb: Kccb::new(),
            irq,
            irq_slot,
            timer,
            waits,
            irqs: 0,
            fence,
            tables: None,
            fw_mem_ctx: None,
            context: None,
        });
        let started = gpu.wait_until(t0 + FW_BOOT_TIMEOUT_NS, |g| {
            g.mem.read_u32(&g.fw.sysinit, fwif::SYSINIT_FIRMWARE_STARTED as u32) != 0
        });
        let boot_us = now_ns().saturating_sub(t0) / 1000;
        let stage = gpu.mem.read_u32(
            &gpu.fw.boot_data,
            nexus_gfx::backend::gpu::mips_boot::BOOT_STAGE_OFFSET as u32,
        ) as i32;
        if !started {
            let (exc, mars, ev, mmu, req, mmuf) = gpu.win.fault_witness();
            emit(&alloc::format!(
                "gpud: FAIL gpu fw (not started in {boot_us}us stage={stage} irqs={} mips_exception={exc:#x} mars_idle={mars:#x} event={ev:#x} bif_mmu={mmu:#x} bif_req={req:#x} mmu_fault={mmuf:#x})",
                gpu.irqs
            ));
            return None;
        }
        // The health check: the first kernel command. The open driver sends it without a return
        // slot and reads `kccb_cmds_executed` (its watchdog's liveness) — so does this rung;
        // the return slot is read as a witness (cycle 2 of 2026-10-13: executed advanced to 1
        // while the slot stayed 0 — the firmware answers a health check without a slot).
        let th0 = now_ns();
        let executed_before =
            gpu.mem.read_u32(&gpu.fw.osdata, fwif::OSDATA_KCCB_CMDS_EXECUTED as u32);
        let health = gpu.kccb_send(&KccbCmd::health_check()).ok();
        let health_ok = health.is_some()
            && gpu.wait_until(th0 + KCCB_TIMEOUT_NS, |g| {
                g.mem.read_u32(&g.fw.osdata, fwif::OSDATA_KCCB_CMDS_EXECUTED as u32)
                    > executed_before
            });
        let health_us = now_ns().saturating_sub(th0) / 1000;
        let fw_irqs = gpu.mem.read_u32(&gpu.fw.osdata, fwif::OSDATA_INTERRUPT_COUNT as u32);
        let executed = gpu.mem.read_u32(&gpu.fw.osdata, fwif::OSDATA_KCCB_CMDS_EXECUTED as u32);
        let rtn = health.map_or(u32::MAX, |slot| gpu.mem.read_u32(&gpu.fw.kccb_rtn, slot * 4));
        if !health_ok {
            emit(&alloc::format!(
                "gpud: FAIL gpu fw (started in {boot_us}us but the health check was not executed in {health_us}us irqs={} fw_irqs={fw_irqs} executed={executed} rtn={rtn:#x} pow={:#x} hwr={:#x})",
                gpu.irqs,
                gpu.mem.read_u32(&gpu.fw.sysdata, fwif::SYSDATA_POW_STATE as u32),
                gpu.mem.read_u32(&gpu.fw.sysdata, fwif::SYSDATA_HWR_STATE_FLAGS as u32)
            ));
            return None;
        }
        emit(&alloc::format!(
            "gpud: gpu fw ok (boot_us={boot_us} health_us={health_us} irqs={} fw_irqs={fw_irqs} executed={executed} rtn={rtn:#x} stage={stage} ts={} pow={:#x} hwr={:#x} version=1.1 build=6976702)",
            gpu.irqs,
            gpu.mem.read_u32(&gpu.fw.sysinit, fwif::SYSINIT_FIRMWARE_STARTED_TIMESTAMP as u32),
            gpu.mem.read_u32(&gpu.fw.sysdata, fwif::SYSDATA_POW_STATE as u32),
            gpu.mem.read_u32(&gpu.fw.sysdata, fwif::SYSDATA_HWR_STATE_FLAGS as u32)
        ));
        emit(&alloc::format!(
            "gpud: gpu caps (va_bits=40 page=4096 clusters={} queues=render+transfer compute=declared cache_block={} backing_free={} id={:#x})",
            gpu.fw.num_clusters,
            gpu.mem.block(),
            gpu.mem.free_pages(),
            id.0
        ));
        if !gpu.mmu_rung() {
            return None;
        }
        job::fence_rung(&mut gpu);
        gpu.timer.disarm();
        let _ = gpu.timer.drain();
        Some(gpu)
    }

    pub(crate) const fn irq_slot(&self) -> u32 {
        self.irq_slot
    }

    /// Acknowledge the firmware processor's interrupt: clear while pending, then complete the
    /// line at the kernel.
    fn ack_irq(&mut self) {
        let mut n = 0;
        while start::irq_pending(&mut self.win) && n < 16 {
            start::irq_clear(&mut self.win);
            n += 1;
        }
        let _ = nexus_abi::irq_complete(self.irq);
    }

    /// One wait on the interrupt endpoint, bounded by the watchdog one-shot armed at
    /// `deadline_ns` (both waitset members; the interrupt ends it early): `true` when the
    /// interrupt fired. Every notification drains the endpoint and acknowledges the device.
    fn wait_slice(&mut self, deadline_ns: u64) -> bool {
        self.timer.arm_at(deadline_ns);
        let fired = match self.waits.wait() {
            Ok(0) => true,
            Ok(_) => {
                let _ = self.timer.drain();
                false
            }
            Err(_) => false,
        };
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut note = [0u8; 16];
        let mut drained = 0u32;
        while nexus_abi::ipc_recv_v1_nb(self.irq_slot, &mut hdr, &mut note, true).is_ok() {
            drained += 1;
        }
        if drained > 0 {
            self.irqs = self.irqs.saturating_add(drained);
            self.ack_irq();
        }
        fired || drained > 0
    }

    /// Waits until `done` holds or `deadline_ns` passes, in interrupt-ended slices.
    pub(super) fn wait_until(
        &mut self,
        deadline_ns: u64,
        mut done: impl FnMut(&Gpu) -> bool,
    ) -> bool {
        loop {
            if done(self) {
                return true;
            }
            let now = now_ns();
            if now >= deadline_ns {
                return done(self);
            }
            let slice = (now + WAIT_SLICE_NS).min(deadline_ns);
            self.wait_slice(slice);
        }
    }

    /// Writes `cmd` into the kernel command buffer, publishes it and kicks the firmware;
    /// returns the slot whose return word says executed.
    pub(super) fn kccb_send(&mut self, cmd: &KccbCmd) -> Result<u32, &'static str> {
        let (ctl, slots, rtn) = (self.fw.kccb_ctl, self.fw.kccb, self.fw.kccb_rtn);
        let mut m = KccbMem { mem: &mut self.mem, ctl, slots, rtn };
        let slot = self.kccb.send(&mut m, cmd).map_err(|_| "kccb full")?;
        start::kick(&mut self.win);
        Ok(slot)
    }

    /// Whether the firmware executed the command in `slot`.
    fn slot_executed(&self, slot: u32) -> bool {
        self.mem.read_u32(&self.fw.kccb_rtn, slot * 4) & fwif::kccb::RTN_SLOT_CMD_EXECUTED != 0
    }

    /// Waits for `slot`'s return word.
    pub(super) fn wait_slot(&mut self, slot: u32, deadline_ns: u64) -> bool {
        self.wait_until(deadline_ns, |g| g.slot_executed(slot))
    }

    /// The service loop's wake on the GPU's endpoint: drain, acknowledge, read the firmware's
    /// messages (a context reset is named), signal what completed.
    pub(crate) fn on_wake(&mut self) {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut note = [0u8; 16];
        let mut drained = 0;
        while nexus_abi::ipc_recv_v1_nb(self.irq_slot, &mut hdr, &mut note, true).is_ok() {
            drained += 1;
        }
        if drained == 0 {
            return;
        }
        self.irqs = self.irqs.saturating_add(drained);
        self.ack_irq();
        self.drain_fwccb();
        job::completions(self);
    }

    /// The firmware's messages: each named once; a request for a restart or a context reset
    /// is a line (the reset path is layer 4's).
    pub(super) fn drain_fwccb(&mut self) {
        let (ctl, slots) = (self.fw.fwccb_ctl, self.fw.fwccb);
        let mut m = FwccbMem { mem: &mut self.mem, ctl, slots };
        let reader = Fwccb;
        let mut n = 0;
        while let Some(cmd) = reader.next(&mut m) {
            n += 1;
            if n > fwif::fwccb::NUM_CMDS {
                break;
            }
            if !cmd.has_magic() {
                emit(&alloc::format!(
                    "gpud: gpu fwccb dropped (type={:#x} without the magic)",
                    cmd.cmd_type
                ));
                continue;
            }
            match cmd.cmd_type {
                fwif::fwccb::CONTEXT_RESET_NOTIFICATION => {
                    if let Some((ctx, reason, dm, job, flags, pc, fault)) = cmd.context_reset() {
                        emit(&alloc::format!(
                            "gpud: FAIL gpu context reset (ctx={ctx} reason={reason} dm={dm} job={job} flags={flags:#x} pc={pc:#x} fault={fault:#x})"
                        ));
                    }
                }
                fwif::fwccb::REQUEST_GPU_RESTART => {
                    emit("gpud: FAIL gpu restart requested by the firmware")
                }
                fwif::fwccb::CONTEXT_FW_PF_NOTIFICATION => {
                    let fault = u64::from_le_bytes(cmd.data[0..8].try_into().unwrap_or([0; 8]));
                    emit(&alloc::format!("gpud: FAIL gpu firmware page fault (addr={fault:#x})"));
                }
                fwif::fwccb::UPDATE_STATS => {}
                other => emit(&alloc::format!(
                    "gpud: gpu fwccb (type={})",
                    other & !fwif::CMD_MAGIC_MASK
                )),
            }
        }
    }

    /// P4: one address space, a page bound, the firmware's MMU cache flushed and acknowledged,
    /// the page unbound and flushed again.
    #[inline(never)]
    fn mmu_rung(&mut self) -> bool {
        let t0 = now_ns();
        let mut tables = match PageTables::new(&mut Tables(&mut self.mem)) {
            Ok(t) => t,
            Err(e) => {
                emit(&alloc::format!("gpud: FAIL gpu mmu (no catalogue page: {e:?})"));
                return false;
            }
        };
        // The firmware memory context names the catalogue; the firmware assigns the slot.
        let Some(ctx) = self.mem.alloc(FwMemContext::SIZE as u32) else {
            emit("gpud: FAIL gpu mmu (no memory for the firmware memory context)");
            return false;
        };
        self.fw.map_object(&mut self.mem, &ctx);
        self.mem.write(&ctx, 0, &FwMemContext::new(tables.root_phys()).to_bytes());
        let Some(page) = self.mem.alloc_page() else {
            emit("gpud: FAIL gpu mmu (no page to bind)");
            return false;
        };
        let flags = PageFlags { read_only: false, slc_bypass: true, pm_fw_protect: false };
        if let Err(e) =
            tables.map(&mut Tables(&mut self.mem), heaps::GENERAL_BASE, page.phys, flags)
        {
            emit(&alloc::format!("gpud: FAIL gpu mmu (map: {e:?})"));
            return false;
        }
        let map_flags = tables.take_flush_flags();
        if !self.mmu_flush(map_flags) {
            emit(&alloc::format!(
                "gpud: FAIL gpu mmu (the flush after a map got no return; flags={map_flags:#x})"
            ));
            return false;
        }
        let map_us = now_ns().saturating_sub(t0) / 1000;
        let t1 = now_ns();
        if let Err(e) = tables.unmap(&mut Tables(&mut self.mem), heaps::GENERAL_BASE) {
            emit(&alloc::format!("gpud: FAIL gpu mmu (unmap: {e:?})"));
            return false;
        }
        let unmap_flags = tables.take_flush_flags();
        if !self.mmu_flush(unmap_flags) {
            emit(&alloc::format!("gpud: FAIL gpu mmu (the flush after an unmap got no return; flags={unmap_flags:#x})"));
            return false;
        }
        let unmap_us = now_ns().saturating_sub(t1) / 1000;
        emit(&alloc::format!(
            "gpud: gpu mmu ok (map_us={map_us} unmap_us={unmap_us} pc={:#x} map_flags={map_flags:#x} unmap_flags={unmap_flags:#x} table_pages={})",
            tables.root_phys(),
            tables.table_pages()
        ));
        self.tables = Some(tables);
        self.fw_mem_ctx = Some(ctx);
        true
    }

    /// The firmware's MMU-cache command, waited on its return slot.
    pub(super) fn mmu_flush(&mut self, flags: u32) -> bool {
        if flags == 0 {
            return true;
        }
        let sync = self.fw.mmucache_sync.fw_addr();
        let t0 = now_ns();
        match self.kccb_send(&KccbCmd::mmu_cache(flags, sync, 0)) {
            Ok(slot) => self.wait_slot(slot, t0 + KCCB_TIMEOUT_NS),
            Err(_) => false,
        }
    }
}
