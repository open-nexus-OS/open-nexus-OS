// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The render queue on the device (TASK-0215 5a, RFC-0101 §8d): what the open kernel driver
//! builds on its side, built here — one render context (the geometry's and the fragment's
//! common contexts in one firmware object with the static context-switch state, two client
//! command buffers, the geometry's call-stack pointer in its context state), the two free lists
//! of parameter memory (the device's own DMA pages, their numbers written into the client's list
//! buffers, the firmware structs the kernel initialises), the render-target dataset (the common
//! block from the attachment's geometry, two render-target data) — and the kick of one pass in
//! the kernel's sequence: geometry `[GEOM (partial-render fence = the fragment queue's next
//! point − 1), UPDATE g]`, fragment `[FENCE_PR (geometry ≥ g), FRAG_PR, UPDATE f1, FRAG, UPDATE
//! f2]`, one kick per queue. Points carry their queue in the high word.

use alloc::vec::Vec;

use nexus_abi::DmaVmo;
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
/// A point's queue, in its high word (the transfer queue is 0: layer 4's points unchanged).
pub(super) const QUEUE_SHIFT: u32 = 32;
pub(super) const RENDER_QUEUE: u64 = 1;
/// The device's ids of the free lists, as the firmware names them in its messages.
const LOCAL_FREELIST_ID: u32 = 1;
const GLOBAL_FREELIST_ID: u32 = 2;

/// One of the render context's queues.
struct Queue {
    fwctx_fw_addr: u32,
    cccb_ctl: HeapObj,
    cccb: HeapObj,
    timeline: HeapObj,
    writer: Cccb,
    seq: u32,
}

/// The device's render objects for one context.
pub(super) struct Render {
    _ctx: HeapObj,
    geom: Queue,
    frag: Queue,
    _freelists: [HeapObj; 2],
    /// The parameter memory behind the lists, alive as long as the lists are.
    _pm: [DmaVmo; 2],
    _common: HeapObj,
    /// The two render-target data: a pass uses `data[pass % 2]`, as the open driver alternates
    /// them frame by frame (the two-frame capture of 2026-10-14: `hwrt=1/0`, then `hwrt=1/1`); a
    /// second pass on the same data locked the fragment stage (5b's probe cycle).
    data: [HeapObj; 2],
    passes: u32,
}

/// What the device must write into the client's list buffers after `create`.
pub(super) struct ListEntries {
    pub args: FreelistArgs,
    pub entries: Vec<u32>,
}

fn fail(what: &'static str) -> DeviceError {
    super::emit(&alloc::format!("gpud: FAIL gpu render ({what})"));
    DeviceError::Lost
}

fn alloc(gpu: &mut Gpu, len: u32, what: &'static str) -> Result<HeapObj, DeviceError> {
    let Some(obj) = gpu.mem.alloc(len) else {
        super::emit(&alloc::format!(
            "gpud: FAIL gpu render ({what}: {len} bytes, backing_free={})",
            gpu.mem.free_pages()
        ));
        return Err(DeviceError::Lost);
    };
    gpu.fw.map_object(&mut gpu.mem, &obj);
    Ok(obj)
}

fn queue(
    gpu: &mut Gpu,
    fwctx_fw_addr: u32,
    state_len: u32,
    what: &'static str,
) -> Result<(Queue, HeapObj), DeviceError> {
    let cccb_ctl = alloc(gpu, CccbCtl::SIZE as u32, what)?;
    let cccb = alloc(gpu, fwif::ccb::SIZE, what)?;
    let timeline = alloc(gpu, 4, what)?;
    let state = alloc(gpu, state_len, what)?;
    let ctl = CccbCtl { wrap_mask: fwif::ccb::SIZE - 1, ..Default::default() };
    gpu.mem.write(&cccb_ctl, 0, &ctl.to_bytes());
    gpu.mem.write_u32(&timeline, 0, 0);
    Ok((Queue { fwctx_fw_addr, cccb_ctl, cccb, timeline, writer: Cccb::new(), seq: 0 }, state))
}

impl Render {
    /// Builds the render context, the free lists and the dataset for `setup` on a core of
    /// `tiles`; returns what the device must write into the list buffers.
    #[inline(never)]
    pub(super) fn create(
        gpu: &mut Gpu,
        setup: &RenderSetup<'_>,
        tiles: TileGeometry,
    ) -> Result<(Self, [ListEntries; 2]), DeviceError> {
        let args = HwrtArgs::parse(setup.hwrt_args)
            .map_err(|_| fail("the dataset's arguments refused"))?;
        for fl in &setup.freelists {
            fl.check(FREELIST_MIN_PAGES)
                .map_err(|_| fail("a free list refused by the kernel's checks"))?;
        }
        let state = StaticRenderContextState::from_stream(setup.static_state)
            .map_err(|_| fail("the static context state refused"))?;
        let fw_mem_ctx =
            gpu.fw_mem_ctx.ok_or_else(|| fail("no firmware memory context"))?.fw_addr();

        // The render context object and its two queues.
        let ctx = alloc(gpu, RENDER_CTX_SIZE, "no memory for the render context")?;
        let (geom, geom_state) =
            queue(gpu, ctx.fw_addr(), GEOM_CTX_STATE, "no memory for the geometry queue")?;
        let (frag, frag_state) = queue(
            gpu,
            ctx.fw_addr() + FwCommonContext::SIZE as u32,
            FRAG_CTX_STATE,
            "no memory for the fragment queue",
        )?;
        // The geometry's call-stack pointer init (core 0, offset 8).
        gpu.mem.write(&geom_state, 8, &setup.callstack_addr.to_le_bytes());
        // One context id for both queues — the kernel's `ctx->ctx_id` (the transfer context is 1).
        for (q, dm, id, st) in
            [(&geom, fwif::dm::GEOM, 2u32, &geom_state), (&frag, fwif::dm::FRAG, 2u32, &frag_state)]
        {
            let cctx = FwCommonContext::for_queue(&QueueInit {
                cccb_ctl_fw_addr: q.cccb_ctl.fw_addr(),
                cccb_fw_addr: q.cccb.fw_addr(),
                dm,
                priority: fwif::priority::HIGH,
                fw_mem_context_fw_addr: fw_mem_ctx,
                context_state_fw_addr: st.fw_addr(),
                server_common_context_id: id,
                pid: 1,
            });
            gpu.mem.write(&ctx, q.fwctx_fw_addr - ctx.fw_addr(), &cctx.to_bytes());
        }
        gpu.mem.write(&ctx, STATIC_STATE_OFFSET, &state.to_fw_bytes());

        // The free lists: the device's pages, the kernel's init.
        let mut pm = Vec::with_capacity(2);
        let mut lists = Vec::with_capacity(2);
        let mut fl_objs = Vec::with_capacity(2);
        for (i, fl) in setup.freelists.iter().enumerate() {
            let bytes = fl.initial_pages as usize * 4096;
            let vmo = DmaVmo::anonymous(DEVICE_MMIO_SLOT, bytes)
                .map_err(|_| fail("no DMA memory for a free list's pages"))?;
            let mut entries = Vec::with_capacity(fl.initial_pages as usize);
            for run in vmo.runs() {
                let n = run.len.div_ceil(4096);
                entries.extend((0..n).map(|p| page_entry(run.bus + p * 4096)));
            }
            entries.truncate(fl.initial_pages as usize);
            if entries.len() != fl.initial_pages as usize {
                return Err(fail("a free list's pages fell short"));
            }
            let id = if i == 0 { LOCAL_FREELIST_ID } else { GLOBAL_FREELIST_ID };
            let obj = alloc(gpu, Freelist::SIZE as u32, "no memory for a free list")?;
            gpu.mem.write(&obj, 0, &Freelist::new(fl, id).to_bytes());
            pm.push(vmo);
            fl_objs.push(obj);
            lists.push(ListEntries { args: *fl, entries });
        }

        // The dataset.
        let common_block = HwrtDataCommon::new(&args, tiles)
            .ok_or_else(|| fail("the dataset's geometry refused"))?;
        let common = alloc(gpu, HwrtDataCommon::SIZE as u32, "no memory for the dataset")?;
        gpu.mem.write(&common, 0, &common_block.to_bytes());
        let fl_addrs = [fl_objs[0].fw_addr(), fl_objs[1].fw_addr()];
        let mut data = Vec::with_capacity(2);
        for i in 0..2 {
            let obj = alloc(gpu, HwrtData::SIZE as u32, "no memory for the render-target data")?;
            gpu.mem.write(&obj, 0, &HwrtData::new(&args, i, common.fw_addr(), fl_addrs).to_bytes());
            data.push(obj);
        }
        // The firmware's MMU cache: the objects above are new pages of its heap.
        if !gpu.mmu_flush(fwif::mmucache::LEVEL_2) {
            return Err(fail("the flush after the render objects got no return"));
        }
        let Ok(pm) = <[DmaVmo; 2]>::try_from(pm) else { return Err(fail("two free lists")) };
        let Ok(fl) = <[HeapObj; 2]>::try_from(fl_objs) else { return Err(fail("two free lists")) };
        let Ok(data) = <[HeapObj; 2]>::try_from(data) else {
            return Err(fail("two render-target data"));
        };
        let Ok(lists) = <[ListEntries; 2]>::try_from(lists) else {
            return Err(fail("two free lists"));
        };
        Ok((
            Self {
                _ctx: ctx,
                geom,
                frag,
                _freelists: fl,
                _pm: pm,
                _common: common,
                data,
                passes: 0,
            },
            lists,
        ))
    }

    /// The fragment queue's timeline word.
    pub(super) fn frag_word(&self, gpu: &Gpu) -> u32 {
        gpu.mem.read_u32(&self.frag.timeline, 0)
    }

    /// The fragment queue's timeline object, for a wait that reads it without `self`.
    pub(super) fn frag_timeline_obj(&self) -> HeapObj {
        self.frag.timeline
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
