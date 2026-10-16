// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The transfer queue, the null job and the fence bridge (TASK-0329 G1 P5, RFC-0101 §8b, TASK-0215
//! 5c-3 P2b): one transfer-fragment firmware context — its client command buffer, its
//! register-state area, its timeline word — on a context's address space, and a job that is one
//! `UPDATE` of that word, kicked through the kernel command buffer with one register write. The
//! firmware writes the word, raises the interrupt, gpud reads the word and signals the kernel
//! timeline fence to it: completion is "word ≥ point", never a record. Sixty-four round trips
//! give the frame's fixed cost on our side; the bar is the stock stack's 0.64 ms
//! (`docs/board/measurements/2026-10-10-gpu-stock-baseline/`). A queue goes by the firmware's
//! cleanup of its common context, then its objects come back.

use nexus_gfx::backend::gpu::ccb::{Cccb, CccbMemory};
use nexus_gfx::backend::gpu::fwif::{self, CccbCtl, FwCommonContext, KccbCmd, QueueInit, Ufo};
use nexus_gfx::backend::gpu::mmu::TableStorage;

use super::device::Gpu;
use super::emit;
use super::memory::{FwMemory, HeapObj};
use super::space::Space;

/// Round trips the rung measures.
const ROUNDS: u32 = 64;
/// One job's budget.
const JOB_TIMEOUT_NS: u64 = 1_000_000_000;
/// The bar: the stock stack's fixed cost of a frame, in microseconds.
const STOCK_FIXED_COST_US: u64 = 640;
/// The fence rung's context id (the device numbers its contexts from the slot and generation;
/// the rung's queue lives before the device and is gone when it starts).
const RUNG_CONTEXT_ID: u32 = 1;

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// One transfer queue's firmware objects.
pub(super) struct Queue {
    pub fwctx: HeapObj,
    pub cccb_ctl: HeapObj,
    pub cccb: HeapObj,
    pub reg_state: HeapObj,
    pub timeline: HeapObj,
    pub writer: Cccb,
    /// The last point submitted.
    pub seq: u32,
    /// The last point signalled on the kernel fence.
    pub signalled: u32,
}

/// A client command buffer's memory: its control block and its ring in the firmware heap.
pub(super) struct CccbMem<'a> {
    pub(super) mem: &'a mut FwMemory,
    pub(super) ctl: HeapObj,
    pub(super) ring: HeapObj,
}

impl CccbMemory for CccbMem<'_> {
    fn read_offset(&self) -> u32 {
        self.mem.read_u32(&self.ctl, CccbCtl::READ_OFFSET as u32)
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        self.mem.write(&self.ring, offset, bytes);
    }
}

impl Queue {
    /// A transfer-fragment queue on the space whose firmware memory context is `fw_mem_ctx`, its
    /// common context numbered `id` (the firmware's messages name it) at `priority`.
    pub(super) fn create(gpu: &mut Gpu, fw_mem_ctx: u32, id: u32, priority: u32) -> Option<Self> {
        let fwctx = gpu.mem.alloc(FwCommonContext::SIZE as u32)?;
        let cccb_ctl = gpu.mem.alloc(CccbCtl::SIZE as u32)?;
        let cccb = gpu.mem.alloc(fwif::ccb::SIZE)?;
        let reg_state = gpu.mem.alloc(fwif::TRANSFER_CTX_STATE_SIZE)?;
        let timeline = gpu.mem.alloc(4)?;
        for obj in [&fwctx, &cccb_ctl, &cccb, &reg_state, &timeline] {
            gpu.fw.map_object(&mut gpu.mem, obj);
        }
        // The open driver's rule: an MMU-cache flush between mapping firmware objects and the
        // kick that reaches them — the firmware's own TLB holds page PAIRS, and a neighbour that
        // was unmapped when its pair was loaded stays invalid until the flush (cycle 3 of
        // 2026-10-13: a firmware page fault, message type 112, on the first kick without it).
        if !gpu.mmu_flush(fwif::mmucache::LEVEL_2) {
            emit("gpud: FAIL gpu queue (the flush after mapping the context got no return)");
            return None;
        }
        let ctl = CccbCtl { wrap_mask: fwif::ccb::SIZE - 1, ..Default::default() };
        gpu.mem.write(&cccb_ctl, 0, &ctl.to_bytes());
        let ctx = FwCommonContext::for_queue(&QueueInit {
            cccb_ctl_fw_addr: cccb_ctl.fw_addr(),
            cccb_fw_addr: cccb.fw_addr(),
            dm: fwif::dm::FRAG,
            priority,
            fw_mem_context_fw_addr: fw_mem_ctx,
            context_state_fw_addr: reg_state.fw_addr(),
            server_common_context_id: id,
            pid: 1,
        });
        gpu.mem.write(&fwctx, 0, &ctx.to_bytes());
        Some(Self {
            fwctx,
            cccb_ctl,
            cccb,
            reg_state,
            timeline,
            writer: Cccb::new(),
            seq: 0,
            signalled: 0,
        })
    }

    /// Submits one task for the next point — the command `cmd_type` with `payload` (the null
    /// job's `NULL` without one, a transfer job's `TQ_3D` with its 192-byte command), then the
    /// `UPDATE` of the timeline word — and kicks once. Returns the point the task will signal.
    pub(super) fn submit_task(
        &mut self,
        gpu: &mut Gpu,
        cmd_type: u32,
        payload: &[u8],
    ) -> Result<u32, &'static str> {
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        let (ctl, ring) = (self.cccb_ctl, self.cccb);
        {
            let mut m = CccbMem { mem: &mut gpu.mem, ctl, ring };
            // H6 (cycles 3–4 of 2026-10-13): a kick with an `UPDATE` alone was never read by
            // the firmware (`cccb_read=0`). A job in the interface is a TASK command plus its
            // update; the interface's `NULL` task (210) is the no-op task — the null job proper.
            self.writer
                .write_command(&mut m, cmd_type, payload, seq, seq)
                .map_err(|_| "cccb full")?;
            self.writer
                .write_update(&mut m, Ufo { addr: self.timeline.fw_addr(), value: seq }, seq)
                .map_err(|_| "cccb full")?;
        }
        let kick = KccbCmd::kick(
            self.fwctx.fw_addr(),
            self.writer.write_offset(),
            self.writer.wrap_mask(),
        );
        gpu.kccb_send(&kick)?;
        Ok(seq)
    }

    /// The timeline word, as the firmware left it.
    pub(super) fn word(&self, gpu: &Gpu) -> u32 {
        gpu.mem.read_u32(&self.timeline, 0)
    }

    /// Signals the kernel fence to the timeline word when it moved.
    fn bridge(&mut self, gpu: &Gpu) {
        let word = self.word(gpu);
        if word != self.signalled && word.wrapping_sub(self.signalled) < 0x8000_0000 {
            let _ = nexus_abi::fence_signal(gpu.fence, u64::from(word));
            self.signalled = word;
        }
    }

    /// The firmware lets go of the queue (its common context's cleanup answered; the caller
    /// waited for its last point), then its objects come back. On a refusal nothing is freed —
    /// the firmware may still reach the objects — and the reason is returned.
    pub(super) fn destroy(self, gpu: &mut Gpu) -> Result<(), &'static str> {
        gpu.cleanup(fwif::cleanup::COMMON_CONTEXT, self.fwctx.fw_addr())?;
        let objs = [self.fwctx, self.cccb_ctl, self.cccb, self.reg_state, self.timeline];
        if gpu.release(&objs) {
            Ok(())
        } else {
            Err("the flush after the cleanup got no return")
        }
    }
}

/// P5: a space and its transfer queue created, sixty-four null jobs round-tripped, the mean
/// against the bar; then the queue's cleanup by the firmware and the space given back, every page
/// of both back in the pool (5c-3 P2b: `cleanup_us=`, `freed=`). A queue whose job did not come
/// back is left to the firmware, never freed.
#[inline(never)]
pub(super) fn fence_rung(gpu: &mut Gpu) {
    let free_before = gpu.mem.free_pages();
    let space = match Space::create(gpu, TableStorage::new()) {
        Ok(s) => s,
        Err((e, _)) => return emit(&alloc::format!("gpud: FAIL gpu fence ({e})")),
    };
    let ctx_mem = space.fw_mem_ctx.fw_addr();
    let Some(mut q) = Queue::create(gpu, ctx_mem, RUNG_CONTEXT_ID, fwif::priority::HIGH) else {
        emit("gpud: FAIL gpu fence (no memory for the queue)");
        return;
    };
    let mut total_ns = 0u64;
    let mut worst_ns = 0u64;
    let irqs_before = gpu.irqs;
    for round in 0..ROUNDS {
        let t0 = now_ns();
        let seq = match q.submit_task(gpu, fwif::ccb::NULL, &[]) {
            Ok(s) => s,
            Err(e) => return emit(&alloc::format!("gpud: FAIL gpu fence (submit {round}: {e})")),
        };
        let done =
            gpu.wait_until(t0 + JOB_TIMEOUT_NS, |g| q.word(g).wrapping_sub(seq) < 0x8000_0000);
        if !done {
            let word = q.word(gpu);
            let read = gpu.mem.read_u32(&q.cccb_ctl, CccbCtl::READ_OFFSET as u32);
            emit(&alloc::format!(
                "gpud: FAIL gpu fence (job {seq} not done in {}us word={word} cccb_read={read} woff={} irqs={} pow={:#x} hwr={:#x} fwctx={:#x} cccb={:#x} ctl={:#x} ufo={:#x})",
                now_ns().saturating_sub(t0) / 1000,
                q.writer.write_offset(),
                gpu.irqs,
                gpu.mem.read_u32(&gpu.fw.sysdata, fwif::SYSDATA_POW_STATE as u32),
                gpu.mem.read_u32(&gpu.fw.sysdata, fwif::SYSDATA_HWR_STATE_FLAGS as u32),
                q.fwctx.fw_addr(),
                q.cccb.fw_addr(),
                q.cccb_ctl.fw_addr(),
                q.timeline.fw_addr()
            ));
            gpu.drain_fwccb();
            return;
        }
        q.bridge(gpu);
        // The kernel fence saw the point: the bridge is whole.
        if nexus_abi::fence_wait(gpu.fence, u64::from(seq), now_ns() + 1_000_000).is_err() {
            emit(&alloc::format!("gpud: FAIL gpu fence (point {seq} not on the kernel fence)"));
            return;
        }
        let dt = now_ns().saturating_sub(t0);
        total_ns += dt;
        worst_ns = worst_ns.max(dt);
    }
    let irqs = gpu.irqs - irqs_before;
    let mean_us = total_ns / u64::from(ROUNDS) / 1000;
    let verdict = if mean_us < STOCK_FIXED_COST_US { "below" } else { "NOT below" };
    let t_clean = now_ns();
    if let Err(e) = q.destroy(gpu) {
        return emit(&alloc::format!("gpud: FAIL gpu fence (the queue's cleanup: {e})"));
    }
    let cleanup_us = now_ns().saturating_sub(t_clean) / 1000;
    let (_, flushed) = space.destroy(gpu);
    let free_after = gpu.mem.free_pages();
    if !flushed || free_after != free_before {
        return emit(&alloc::format!(
            "gpud: FAIL gpu fence (the space was not given back: flushed={flushed} free={free_after} of {free_before})"
        ));
    }
    emit(&alloc::format!(
        "gpud: gpu fence ok (rt_us={mean_us} worst_us={} n={ROUNDS} irqs={irqs} {verdict} the stock {STOCK_FIXED_COST_US}us cleanup_us={cleanup_us} freed={free_after}/{free_before})",
        worst_ns / 1000
    ));
}
