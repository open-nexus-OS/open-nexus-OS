// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The null job and the fence bridge (TASK-0329 G1 P5, RFC-0101 §5/§8b): one transfer-fragment
//! firmware context — its client command buffer, its register-state area, its timeline word —
//! and a job that is one `UPDATE` of that word, kicked through the kernel command buffer with
//! one register write. The firmware writes the word, raises the interrupt, gpud reads the word
//! and signals the kernel timeline fence to it: completion is "word ≥ point", never a record.
//! Sixty-four round trips give the frame's fixed cost on our side; the bar is the stock
//! stack's 0.64 ms (`docs/board/measurements/2026-10-10-gpu-stock-baseline/`).

use nexus_gfx::backend::gpu::ccb::{Cccb, CccbMemory};
use nexus_gfx::backend::gpu::fwif::{self, CccbCtl, FwCommonContext, KccbCmd, QueueInit, Ufo};

use super::device::Gpu;
use super::emit;
use super::memory::{FwMemory, HeapObj};

/// Round trips the rung measures.
const ROUNDS: u32 = 64;
/// One job's budget.
const JOB_TIMEOUT_NS: u64 = 1_000_000_000;
/// The bar: the stock stack's fixed cost of a frame, in microseconds.
const STOCK_FIXED_COST_US: u64 = 640;

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// One queue's firmware objects.
pub(super) struct Context {
    pub fwctx: HeapObj,
    pub cccb_ctl: HeapObj,
    pub cccb: HeapObj,
    pub timeline: HeapObj,
    pub writer: Cccb,
    /// The last point submitted.
    pub seq: u32,
    /// The last point signalled on the kernel fence.
    pub signalled: u32,
}

struct CccbMem<'a> {
    mem: &'a mut FwMemory,
    ctl: HeapObj,
    ring: HeapObj,
}

impl CccbMemory for CccbMem<'_> {
    fn read_offset(&self) -> u32 {
        self.mem.read_u32(&self.ctl, CccbCtl::READ_OFFSET as u32)
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        self.mem.write(&self.ring, offset, bytes);
    }
}

/// A transfer-fragment context in the bring-up's address space.
pub(super) fn create(gpu: &mut Gpu) -> Option<Context> {
    let fw_mem_ctx = gpu.fw_mem_ctx?.fw_addr();
    let fwctx = gpu.mem.alloc(FwCommonContext::SIZE as u32)?;
    let cccb_ctl = gpu.mem.alloc(CccbCtl::SIZE as u32)?;
    let cccb = gpu.mem.alloc(fwif::ccb::SIZE)?;
    let reg_state = gpu.mem.alloc(fwif::TRANSFER_CTX_STATE_SIZE)?;
    let timeline = gpu.mem.alloc(4)?;
    for obj in [&fwctx, &cccb_ctl, &cccb, &reg_state, &timeline] {
        gpu.fw.map_object(&mut gpu.mem, obj);
    }
    // The open driver's rule: an MMU-cache flush between mapping firmware objects and the kick
    // that reaches them — the firmware's own TLB holds page PAIRS, and a neighbour that was
    // unmapped when its pair was loaded stays invalid until the flush (cycle 3 of 2026-10-13:
    // a firmware page fault, message type 112, on the first kick without it).
    if !gpu.mmu_flush(fwif::mmucache::LEVEL_2) {
        emit("gpud: FAIL gpu fence (the flush after mapping the context got no return)");
        return None;
    }
    let ctl = CccbCtl { wrap_mask: fwif::ccb::SIZE - 1, ..Default::default() };
    gpu.mem.write(&cccb_ctl, 0, &ctl.to_bytes());
    let ctx = FwCommonContext::for_queue(&QueueInit {
        cccb_ctl_fw_addr: cccb_ctl.fw_addr(),
        cccb_fw_addr: cccb.fw_addr(),
        dm: fwif::dm::FRAG,
        priority: fwif::priority::HIGH,
        fw_mem_context_fw_addr: fw_mem_ctx,
        context_state_fw_addr: reg_state.fw_addr(),
        server_common_context_id: 1,
        pid: 1,
    });
    gpu.mem.write(&fwctx, 0, &ctx.to_bytes());
    Some(Context { fwctx, cccb_ctl, cccb, timeline, writer: Cccb::new(), seq: 0, signalled: 0 })
}

/// Submits one task for the next point — the command `cmd_type` with `payload` (the null job's
/// `NULL` without one, a transfer job's `TQ_3D` with its 192-byte command), then the `UPDATE`
/// of the timeline word — and kicks once. Returns the point the task will signal.
pub(super) fn submit_task(
    gpu: &mut Gpu,
    ctx: &mut Context,
    cmd_type: u32,
    payload: &[u8],
) -> Result<u32, &'static str> {
    ctx.seq = ctx.seq.wrapping_add(1);
    let seq = ctx.seq;
    let (ctl, ring) = (ctx.cccb_ctl, ctx.cccb);
    {
        let mut m = CccbMem { mem: &mut gpu.mem, ctl, ring };
        // H6 (cycles 3–4 of 2026-10-13): a kick with an `UPDATE` alone was never read by the
        // firmware (`cccb_read=0`). A job in the interface is a TASK command plus its update;
        // the interface's `NULL` task (210) is the no-op task — the null job proper.
        ctx.writer.write_command(&mut m, cmd_type, payload, seq, seq).map_err(|_| "cccb full")?;
        ctx.writer
            .write_update(&mut m, Ufo { addr: ctx.timeline.fw_addr(), value: seq }, seq)
            .map_err(|_| "cccb full")?;
    }
    let kick =
        KccbCmd::kick(ctx.fwctx.fw_addr(), ctx.writer.write_offset(), ctx.writer.wrap_mask());
    gpu.kccb_send(&kick)?;
    Ok(seq)
}

/// The timeline word, as the firmware left it.
pub(super) fn timeline(gpu: &Gpu, ctx: &Context) -> u32 {
    gpu.mem.read_u32(&ctx.timeline, 0)
}

/// Signals the kernel fence to the timeline word when it moved (the service loop's wake and
/// the rung's wait).
pub(super) fn bridge(gpu: &mut Gpu, ctx: &mut Context) {
    let word = timeline(gpu, ctx);
    if word != ctx.signalled && word.wrapping_sub(ctx.signalled) < 0x8000_0000 {
        let _ = nexus_abi::fence_signal(gpu.fence, u64::from(word));
        ctx.signalled = word;
    }
}

/// The service loop's wake: completions since the last wake are bridged to the fence.
pub(super) fn completions(gpu: &mut Gpu) {
    if let Some(mut ctx) = gpu.context.take() {
        bridge(gpu, &mut ctx);
        gpu.context = Some(ctx);
    }
}

/// P5: the context created, sixty-four null jobs round-tripped, the mean against the bar.
#[inline(never)]
pub(super) fn fence_rung(gpu: &mut Gpu) {
    let Some(mut ctx) = create(gpu) else {
        emit("gpud: FAIL gpu fence (no memory for the context)");
        return;
    };
    let mut total_ns = 0u64;
    let mut worst_ns = 0u64;
    let mut irqs_before = gpu.irqs;
    for round in 0..ROUNDS {
        let t0 = now_ns();
        let seq = match submit_task(gpu, &mut ctx, fwif::ccb::NULL, &[]) {
            Ok(s) => s,
            Err(e) => {
                emit(&alloc::format!("gpud: FAIL gpu fence (submit {round}: {e})"));
                gpu.context = Some(ctx);
                return;
            }
        };
        let done = gpu
            .wait_until(t0 + JOB_TIMEOUT_NS, |g| timeline(g, &ctx).wrapping_sub(seq) < 0x8000_0000);
        if !done {
            let word = timeline(gpu, &ctx);
            let read = gpu.mem.read_u32(&ctx.cccb_ctl, CccbCtl::READ_OFFSET as u32);
            emit(&alloc::format!(
                "gpud: FAIL gpu fence (job {seq} not done in {}us word={word} cccb_read={read} woff={} irqs={} pow={:#x} hwr={:#x} fwctx={:#x} cccb={:#x} ctl={:#x} ufo={:#x})",
                now_ns().saturating_sub(t0) / 1000,
                ctx.writer.write_offset(),
                gpu.irqs,
                gpu.mem.read_u32(&gpu.fw.sysdata, fwif::SYSDATA_POW_STATE as u32),
                gpu.mem.read_u32(&gpu.fw.sysdata, fwif::SYSDATA_HWR_STATE_FLAGS as u32),
                ctx.fwctx.fw_addr(),
                ctx.cccb.fw_addr(),
                ctx.cccb_ctl.fw_addr(),
                ctx.timeline.fw_addr()
            ));
            gpu.drain_fwccb();
            gpu.context = Some(ctx);
            return;
        }
        bridge(gpu, &mut ctx);
        // The kernel fence saw the point: the bridge is whole.
        if nexus_abi::fence_wait(gpu.fence, u64::from(seq), now_ns() + 1_000_000).is_err() {
            emit(&alloc::format!("gpud: FAIL gpu fence (point {seq} not on the kernel fence)"));
            gpu.context = Some(ctx);
            return;
        }
        let dt = now_ns().saturating_sub(t0);
        total_ns += dt;
        worst_ns = worst_ns.max(dt);
    }
    let irqs = gpu.irqs - irqs_before;
    irqs_before = gpu.irqs;
    let _ = irqs_before;
    let mean_us = total_ns / u64::from(ROUNDS) / 1000;
    let verdict = if mean_us < STOCK_FIXED_COST_US { "below" } else { "NOT below" };
    emit(&alloc::format!(
        "gpud: gpu fence ok (rt_us={mean_us} worst_us={} n={ROUNDS} irqs={irqs} {verdict} the stock {STOCK_FIXED_COST_US}us)",
        worst_ns / 1000
    ));
    gpu.context = Some(ctx);
}
