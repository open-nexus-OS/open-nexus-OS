// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The render queue on the device (TASK-0215 5a, RFC-0101 §8d): what the open kernel driver
//! builds on its side, built here per context — one render context (the geometry's and the
//! fragment's common contexts in one firmware object with the static context-switch state, two
//! client command buffers, the geometry's call-stack pointer in its context state), the two free
//! lists of parameter memory (the device's own pages — the GPU's alone, no region of gpud's
//! address space — their numbers written into the client's list buffers, the firmware structs
//! the kernel initialises), the render-target dataset (the common block from the attachment's
//! geometry, two render-target data) — and the kick of one pass in the kernel's sequence:
//! geometry `[GEOM (partial-render fence = the fragment queue's next point − 1), UPDATE g]`,
//! fragment `[FENCE_PR (geometry ≥ g), FRAG_PR, UPDATE f1, FRAG, UPDATE f2]`, one kick per
//! queue. A context's render objects go in the open kernel driver's order (5c-3 P2b): the two
//! common contexts, the two render-target data, the free lists — each by the firmware's
//! cleanup — then their memory; nothing is taken from gpud's heap, which never frees.

use nexus_gfx::backend::gpu::ccb::Cccb;
use nexus_gfx::backend::gpu::fwif::{
    self, frag_flags, page_entry, CccbCtl, FragCmd, Freelist, FreelistArgs, FwCommonContext,
    GeomCmd, HwrtArgs, HwrtData, HwrtDataCommon, KccbCmd, QueueInit, RenderFeatures,
    StaticRenderContextState, TileGeometry, Ufo,
};
use nexus_gfx::backend::gpu::scene::RenderSetup;
use nexus_gfx::backend::v2::{DeviceError, RenderStreams};
use nexus_service_topology::DEVICE_MMIO_SLOT;

use super::device::Gpu;
use super::gpu_vmo::GpuVmo;
use super::job::CccbMem;
use super::memory::HeapObj;

/// The geometry context state: four cores of 40 bytes (the call-stack pointer's init at 8).
const GEOM_CTX_STATE: u32 = 160;
/// The fragment context state: 12 bytes and one ISP store word per raster pipe times
/// (1 + the XPU slaves) — four on this core — rounded to 8.
const FRAG_CTX_STATE: u32 = 32;
/// The render context object: two common contexts, the static state, two words.
const STATIC_STATE_OFFSET: u32 = 2 * FwCommonContext::SIZE as u32;
const RENDER_CTX_SIZE: u32 = STATIC_STATE_OFFSET + StaticRenderContextState::FW_SIZE as u32 + 8;
/// The core's minimum for the local free list (roguexe).
const FREELIST_MIN_PAGES: u32 = 25;
/// The device's ids of the free lists, as the firmware names them in its messages.
const LOCAL_FREELIST_ID: u32 = 1;
const GLOBAL_FREELIST_ID: u32 = 2;
/// Firmware objects of one render context: the context, four per queue, two free lists, the
/// common block, two render-target data.
const OBJECTS: usize = 1 + 2 * 4 + 2 + 1 + 2;

/// One of the render context's queues.
struct Queue {
    fwctx_fw_addr: u32,
    cccb_ctl: HeapObj,
    cccb: HeapObj,
    timeline: HeapObj,
    state: HeapObj,
    writer: Cccb,
    seq: u32,
}

/// The device's render objects for one context.
pub(super) struct Render {
    ctx: HeapObj,
    geom: Queue,
    frag: Queue,
    freelists: [HeapObj; 2],
    /// The parameter memory behind the lists, alive as long as the lists are.
    pm: [GpuVmo; 2],
    args: [FreelistArgs; 2],
    common: HeapObj,
    /// The two render-target data: a pass uses `data[pass % 2]`, as the open driver alternates
    /// them frame by frame (the two-frame capture of 2026-10-14: `hwrt=1/0`, then `hwrt=1/1`); a
    /// second pass on the same data locked the fragment stage (5b's probe cycle).
    data: [HeapObj; 2],
    passes: u32,
}

fn fail(what: &'static str) -> DeviceError {
    super::emit(&alloc::format!("gpud: FAIL gpu render ({what})"));
    DeviceError::Lost
}

/// The objects a creation took so far: given back at once when it fails (the firmware has seen
/// none of them).
struct Taken {
    objs: [Option<HeapObj>; OBJECTS],
}

impl Taken {
    fn alloc(
        &mut self,
        gpu: &mut Gpu,
        len: u32,
        what: &'static str,
    ) -> Result<HeapObj, DeviceError> {
        let slot = self.objs.iter().position(Option::is_none).ok_or_else(|| fail(what))?;
        let Some(obj) = gpu.mem.alloc(len) else {
            super::emit(&alloc::format!(
                "gpud: FAIL gpu render ({what}: {len} bytes, backing_free={})",
                gpu.mem.free_pages()
            ));
            return Err(DeviceError::Exhausted);
        };
        gpu.fw.map_object(&mut gpu.mem, &obj);
        self.objs[slot] = Some(obj);
        Ok(obj)
    }

    /// Gives back everything taken (a refused creation).
    fn undo(&self, gpu: &mut Gpu) {
        let mut list = [HeapObj { off: 0, len: 0 }; OBJECTS];
        let mut n = 0;
        for obj in self.objs.iter().flatten() {
            list[n] = *obj;
            n += 1;
        }
        gpu.release(&list[..n]);
    }
}

fn queue(
    gpu: &mut Gpu,
    taken: &mut Taken,
    fwctx_fw_addr: u32,
    state_len: u32,
    what: &'static str,
) -> Result<Queue, DeviceError> {
    let cccb_ctl = taken.alloc(gpu, CccbCtl::SIZE as u32, what)?;
    let cccb = taken.alloc(gpu, fwif::ccb::SIZE, what)?;
    let timeline = taken.alloc(gpu, 4, what)?;
    let state = taken.alloc(gpu, state_len, what)?;
    let ctl = CccbCtl { wrap_mask: fwif::ccb::SIZE - 1, ..Default::default() };
    gpu.mem.write(&cccb_ctl, 0, &ctl.to_bytes());
    gpu.mem.write_u32(&timeline, 0, 0);
    Ok(Queue { fwctx_fw_addr, cccb_ctl, cccb, timeline, state, writer: Cccb::new(), seq: 0 })
}

impl Render {
    /// Builds the render context, the free lists and the dataset for `setup` on a core of
    /// `tiles`, on the space whose firmware memory context is `fw_mem_ctx`, its common contexts
    /// numbered `id` at `priority`. The list entries are written afterwards by the device
    /// ([`Self::entries`]).
    #[inline(never)]
    pub(super) fn create(
        gpu: &mut Gpu,
        setup: &RenderSetup<'_>,
        tiles: TileGeometry,
        fw_mem_ctx: u32,
        id: u32,
        priority: u32,
    ) -> Result<Self, DeviceError> {
        let args = HwrtArgs::parse(setup.hwrt_args)
            .map_err(|_| fail("the dataset's arguments refused"))?;
        for fl in &setup.freelists {
            fl.check(FREELIST_MIN_PAGES)
                .map_err(|_| fail("a free list refused by the kernel's checks"))?;
        }
        let state = StaticRenderContextState::from_stream(setup.static_state)
            .map_err(|_| fail("the static context state refused"))?;
        let common_block = HwrtDataCommon::new(&args, tiles)
            .ok_or_else(|| fail("the dataset's geometry refused"))?;
        // The parameter memory first: no firmware object is taken for a refused one.
        let pm_of =
            |fl: &FreelistArgs| GpuVmo::new(DEVICE_MMIO_SLOT, fl.initial_pages as usize * 4096);
        let (Some(pm0), Some(pm1)) = (pm_of(&setup.freelists[0]), pm_of(&setup.freelists[1]))
        else {
            return Err(fail("no memory for a free list's pages"));
        };
        let mut taken = Taken { objs: [None; OBJECTS] };
        match Self::objects(
            gpu,
            &mut taken,
            setup,
            &args,
            &state,
            &common_block,
            fw_mem_ctx,
            id,
            priority,
        ) {
            Ok((ctx, geom, frag, freelists, common, data)) => Ok(Self {
                ctx,
                geom,
                frag,
                freelists,
                pm: [pm0, pm1],
                args: setup.freelists,
                common,
                data,
                passes: 0,
            }),
            Err(e) => {
                taken.undo(gpu);
                Err(e)
            }
        }
    }

    /// The firmware objects of [`Self::create`], each in `taken`.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn objects(
        gpu: &mut Gpu,
        taken: &mut Taken,
        setup: &RenderSetup<'_>,
        args: &HwrtArgs,
        state: &StaticRenderContextState,
        common_block: &HwrtDataCommon,
        fw_mem_ctx: u32,
        id: u32,
        priority: u32,
    ) -> Result<(HeapObj, Queue, Queue, [HeapObj; 2], HeapObj, [HeapObj; 2]), DeviceError> {
        // The render context object and its two queues.
        let ctx = taken.alloc(gpu, RENDER_CTX_SIZE, "no memory for the render context")?;
        let geom =
            queue(gpu, taken, ctx.fw_addr(), GEOM_CTX_STATE, "no memory for the geometry queue")?;
        let frag = queue(
            gpu,
            taken,
            ctx.fw_addr() + FwCommonContext::SIZE as u32,
            FRAG_CTX_STATE,
            "no memory for the fragment queue",
        )?;
        // The geometry's call-stack pointer init (core 0, offset 8).
        gpu.mem.write(&geom.state, 8, &setup.callstack_addr.to_le_bytes());
        // One context id for both queues — the kernel's `ctx->ctx_id`.
        for (q, dm) in [(&geom, fwif::dm::GEOM), (&frag, fwif::dm::FRAG)] {
            let cctx = FwCommonContext::for_queue(&QueueInit {
                cccb_ctl_fw_addr: q.cccb_ctl.fw_addr(),
                cccb_fw_addr: q.cccb.fw_addr(),
                dm,
                priority,
                fw_mem_context_fw_addr: fw_mem_ctx,
                context_state_fw_addr: q.state.fw_addr(),
                server_common_context_id: id,
                pid: 1,
            });
            gpu.mem.write(&ctx, q.fwctx_fw_addr - ctx.fw_addr(), &cctx.to_bytes());
        }
        gpu.mem.write(&ctx, STATIC_STATE_OFFSET, &state.to_fw_bytes());
        // The free lists' firmware structs, the kernel's init.
        let mut freelists = [HeapObj { off: 0, len: 0 }; 2];
        for (i, fl) in setup.freelists.iter().enumerate() {
            let list_id = if i == 0 { LOCAL_FREELIST_ID } else { GLOBAL_FREELIST_ID };
            let obj = taken.alloc(gpu, Freelist::SIZE as u32, "no memory for a free list")?;
            gpu.mem.write(&obj, 0, &Freelist::new(fl, list_id).to_bytes());
            freelists[i] = obj;
        }
        // The dataset.
        let common = taken.alloc(gpu, HwrtDataCommon::SIZE as u32, "no memory for the dataset")?;
        gpu.mem.write(&common, 0, &common_block.to_bytes());
        let fl_addrs = [freelists[0].fw_addr(), freelists[1].fw_addr()];
        let mut data = [HeapObj { off: 0, len: 0 }; 2];
        for (i, d) in data.iter_mut().enumerate() {
            let obj =
                taken.alloc(gpu, HwrtData::SIZE as u32, "no memory for the render-target data")?;
            gpu.mem.write(&obj, 0, &HwrtData::new(args, i, common.fw_addr(), fl_addrs).to_bytes());
            *d = obj;
        }
        // The firmware's MMU cache: the objects above are new pages of its heap.
        if !gpu.mmu_flush(fwif::mmucache::LEVEL_2) {
            return Err(fail("the flush after the render objects got no return"));
        }
        Ok((ctx, geom, frag, freelists, common, data))
    }

    /// Calls `entry` with free list `list`'s arguments, each entry's index in the list buffer
    /// and its value — the page numbers of the list's parameter pages, the list filled from its
    /// end (`first_entry`); `false` when a page was missing or `entry` refused.
    pub(super) fn entries(
        &self,
        list: usize,
        mut entry: impl FnMut(&FreelistArgs, u32, u32) -> bool,
    ) -> bool {
        let (Some(args), Some(pm)) = (self.args.get(list), self.pm.get(list)) else { return false };
        let mut i = 0u32;
        let walked = pm.walk(DEVICE_MMIO_SLOT, 0, pm.len(), |bus| {
            let ok = i < args.initial_pages && entry(args, args.first_entry() + i, page_entry(bus));
            i += 1;
            ok
        });
        walked && i == args.initial_pages
    }

    /// The context's render objects go, in the open kernel driver's order: each common context's
    /// cleanup (the caller waited for their last points), each render-target data's, each free
    /// list's; then every object's memory and the parameter pages. On a refusal nothing is freed
    /// — the firmware may still reach any of it — and the reason is returned.
    pub(super) fn destroy(self, gpu: &mut Gpu) -> Result<(), &'static str> {
        use fwif::cleanup::{COMMON_CONTEXT, FREELIST, HWRT_DATA};
        let order = [
            (COMMON_CONTEXT, self.frag.fwctx_fw_addr),
            (COMMON_CONTEXT, self.geom.fwctx_fw_addr),
            (HWRT_DATA, self.data[1].fw_addr()),
            (HWRT_DATA, self.data[0].fw_addr()),
            (FREELIST, self.freelists[0].fw_addr()),
            (FREELIST, self.freelists[1].fw_addr()),
        ];
        for (kind, addr) in order {
            if let Err(e) = gpu.cleanup(kind, addr) {
                // The parameter pages stay the device's for good: the firmware may still use them.
                core::mem::forget(self);
                return Err(e);
            }
        }
        let objs = [
            self.ctx,
            self.geom.cccb_ctl,
            self.geom.cccb,
            self.geom.timeline,
            self.geom.state,
            self.frag.cccb_ctl,
            self.frag.cccb,
            self.frag.timeline,
            self.frag.state,
            self.freelists[0],
            self.freelists[1],
            self.common,
            self.data[0],
            self.data[1],
        ];
        if !gpu.release(&objs) {
            core::mem::forget(self);
            return Err("the flush after the cleanup got no return");
        }
        Ok(())
    }

    /// The fragment queue's timeline word.
    pub(super) fn frag_word(&self, gpu: &Gpu) -> u32 {
        gpu.mem.read_u32(&self.frag.timeline, 0)
    }

    /// The two queues' timeline objects with their last points `(geometry, fragment)`.
    pub(super) fn progress(&self) -> ((HeapObj, u32), (HeapObj, u32)) {
        ((self.geom.timeline, self.geom.seq), (self.frag.timeline, self.frag.seq))
    }

    /// The geometry queue's timeline word.
    pub(super) fn geom_word(&self, gpu: &Gpu) -> u32 {
        gpu.mem.read_u32(&self.geom.timeline, 0)
    }

    /// Writes one pass into both queues in the kernel's sequence and kicks them; returns the
    /// fragment point the pass signals.
    pub(super) fn submit(
        &mut self,
        gpu: &mut Gpu,
        s: &RenderStreams<'_>,
        feats: RenderFeatures,
    ) -> Result<u32, DeviceError> {
        let g = self.geom.seq.wrapping_add(1);
        let f1 = self.frag.seq.wrapping_add(1);
        let f2 = self.frag.seq.wrapping_add(2);
        let pr_fence = Ufo { addr: self.frag.timeline.fw_addr(), value: f1.wrapping_sub(1) };
        let hwrt = self.data[(self.passes % 2) as usize].fw_addr();
        self.passes = self.passes.wrapping_add(1);
        let geom = GeomCmd::from_stream(s.geom, feats, s.geom_flags, hwrt, pr_fence)
            .map_err(|_| DeviceError::MalformedStream)?;
        let frag_pr = FragCmd::from_stream(s.frag_pr, feats, s.frag_pr_flags, hwrt)
            .map_err(|_| DeviceError::MalformedStream)?;
        let frag = FragCmd::from_stream(s.frag, feats, s.frag_flags, hwrt)
            .map_err(|_| DeviceError::MalformedStream)?;
        if s.frag_pr_flags & frag_flags::SUBMIT_PARTIAL_RENDER == 0
            || s.frag_flags & frag_flags::SUBMIT_PARTIAL_RENDER != 0
        {
            return Err(DeviceError::MalformedStream);
        }
        let geom_tl = Ufo { addr: self.geom.timeline.fw_addr(), value: g };
        {
            let mut m =
                CccbMem { mem: &mut gpu.mem, ctl: self.geom.cccb_ctl, ring: self.geom.cccb };
            self.geom
                .writer
                .write_command(&mut m, fwif::ccb::GEOM, &geom.to_bytes(), g, g)
                .map_err(|_| DeviceError::Exhausted)?;
            self.geom
                .writer
                .write_update(&mut m, geom_tl, g)
                .map_err(|_| DeviceError::Exhausted)?;
        }
        {
            let mut m =
                CccbMem { mem: &mut gpu.mem, ctl: self.frag.cccb_ctl, ring: self.frag.cccb };
            let w = &mut self.frag.writer;
            w.write_fence(&mut m, &[geom_tl], f1).map_err(|_| DeviceError::Exhausted)?;
            w.write_command(&mut m, fwif::ccb::FRAG_PR, &frag_pr.to_bytes(), f1, f1)
                .map_err(|_| DeviceError::Exhausted)?;
            w.write_update(&mut m, Ufo { addr: self.frag.timeline.fw_addr(), value: f1 }, f1)
                .map_err(|_| DeviceError::Exhausted)?;
            w.write_command(&mut m, fwif::ccb::FRAG, &frag.to_bytes(), f2, f2)
                .map_err(|_| DeviceError::Exhausted)?;
            w.write_update(&mut m, Ufo { addr: self.frag.timeline.fw_addr(), value: f2 }, f2)
                .map_err(|_| DeviceError::Exhausted)?;
        }
        self.geom.seq = g;
        self.frag.seq = f2;
        // Every kick on the data names its cleanup control (the open kernel driver's kick).
        let cleanup = hwrt + HwrtData::CLEANUP_OFFSET as u32;
        for q in [&self.geom, &self.frag] {
            let kick = KccbCmd::kick_with_cleanup(
                q.fwctx_fw_addr,
                q.writer.write_offset(),
                q.writer.wrap_mask(),
                cleanup,
            );
            gpu.kccb_send(&kick).map_err(|_| DeviceError::Lost)?;
        }
        Ok(f2)
    }
}
