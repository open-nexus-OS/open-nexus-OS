// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The desktop on the board's GPU (TASK-0215 5c-3 P4b, RFC-0101 §8d): windowd's presents drawn
//! in windowd's context from the frame planner's steps lowered for the board
//! (`nexus_gfx::frame::board`) — per glass to blur, the scene below it into the scratch and the
//! two blur passes into its cache rows; then the slot's pass, the whole scene into the ring's
//! slot. The context takes once what the frames touch: the shared layout as textures (the
//! wallpaper plane and the atlas, read only), the ring's three slots as targets, the GPU's own
//! scratches and glass cache, and a buffer of its own for the passes' push constants,
//! descriptors, streams and vertices (the bundle's tails hold a few dozen draws, a desktop frame
//! a few hundred). The CPU wrote the textures and the device does not snoop: the base plane (the
//! wallpaper plane; the retained plane while a capture freeze holds the frame) — first, and
//! whenever the CPU rewrote it — and the atlas rows of a new content generation, or the rows a
//! scroll shows new, are cleaned out of the caches before a frame reads them. A frame waits for
//! its passes; the display takes it at its next frame boundary. Lines: `gpud: gpu desktop ok
//! (…)` for the first frame, `gpud: gpu frame us (…)` a window of 32 frames; a refusal is the
//! caller's to say.

use nexus_display_proto::layout::{self, PLANE_BYTES, PLANE_ROWS, RING_ROWS, STRIDE_BYTES};
use nexus_driverkit::gpu::{Access, CacheMode, GpuVa};
use nexus_gfx::backend::gpu::pipelines::Placed;
use nexus_gfx::backend::gpu::quads::{Arena, Rings};
use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{
    BoId, ClientId, ContextId, DeviceError, GfxDevice, Mapping, RenderStreams,
};
use nexus_gfx::command::buffer::Command;
use nexus_gfx::frame::{Area, Board, CacheStep, Frame, History, Pipe, Target as To, BLUR_RADII};

use super::desktop_pass::{clean_rows, run_pass};
use super::device_v2::Device;
use super::emit_fmt;
use super::render_template::{
    render_streams, Captured, Target, Targets, RENDER_BUNDLE, TARGET_PDS,
};
use crate::gpu_context::FrameTarget;

/// Where the context takes what the frames touch: one GiB of GPU addresses (one directory of
/// the context's page tables), clear of the render bundle's heaps. The layout at its base — its
/// planes and atlas read-only, the ring's three slots read-write, one range each, so no page is
/// mapped twice —, then the scratches, the cache and the frame's own buffer, each on a 2 MiB
/// boundary (a page table each such span).
pub(super) const LAYOUT_VA: u64 = 0x20_0000_0000;
pub(super) const SCRATCH_VA: u64 = LAYOUT_VA + 0x0800_0000;
pub(super) const HALF_VA: u64 = LAYOUT_VA + 0x0A00_0000;
pub(super) const CACHE_VA: u64 = LAYOUT_VA + 0x0C00_0000;
pub(super) const FRAME_VA: u64 = LAYOUT_VA + 0x0E00_0000;
const WIDTH: u32 = STRIDE_BYTES / 4;
const STRIDE: u64 = STRIDE_BYTES as u64;
/// The half-resolution scratch's rows: the tallest glass's half and the vertical taps' halo.
pub(crate) const HALF_ROWS: u32 = 600;
/// The glass cache's half-resolution rows — inside the render target's 1080 (a pass draws only
/// on the dataset's screen).
pub(crate) const CACHE_ROWS: u32 = 1024;
/// The frame's buffer: push constants, PPP blocks and descriptors; the control streams; the
/// vertices.
/// A pass of `MAX_DRAWS` takes about 290 bytes of general memory a draw (push constants, the PPP
/// blocks, the descriptor): 640 draws fit with room for the pass before it.
const GENERAL_BYTES: u32 = 256 * 1024;
const STREAM_BYTES: u32 = 32 * 1024;
const VERTEX_BYTES: u32 = 96 * 1024;
const FRAME_BYTES: u32 = GENERAL_BYTES + STREAM_BYTES + VERTEX_BYTES;
/// The PDS heap's lines after the targets' pixel event data, to the bundle's PDS arena's end.
const PDS_END: u32 = 0x2_1000;
/// The most draws a pass holds: the planner's 64 layers (RFC-0109), each up to nine draws, and
/// the wallpaper — a plan can never ask for more, so a pass is never refused for its size.
pub(super) const MAX_DRAWS: usize = 640;
pub(super) const PASS_TIMEOUT_NS: u64 = 500_000_000;
/// Frames in a `gpud: gpu frame us` window.
const WINDOW: u32 = 32;

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// Why the desktop refused: the step, and the device's word when it said one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Refusal(pub &'static str, pub Option<DeviceError>);

impl From<&'static str> for Refusal {
    fn from(step: &'static str) -> Self {
        Self(step, None)
    }
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.1 {
            Some(e) => write!(f, "{} {e:?}", self.0),
            None => f.write_str(self.0),
        }
    }
}

/// The blur passes' names in the library, by [`BLUR_RADII`]'s order.
const ACROSS: [&str; 4] =
    ["blur-horizontal-8", "blur-horizontal-20", "blur-horizontal-30", "blur-horizontal-40"];
const DOWN: [&str; 4] =
    ["blur-vertical-8", "blur-vertical-20", "blur-vertical-30", "blur-vertical-40"];

/// The library's pipelines a frame draws with.
pub(super) struct Pipes {
    pub(super) opaque: Placed,
    straight: Placed,
    rounded: Placed,
    shadow: Placed,
    across: [Placed; 4],
    down: [Placed; 4],
}

impl Pipes {
    fn named(dev: &Device, client: ClientId, ctx: ContextId) -> Result<Self, &'static str> {
        let get = |name: &str| dev.pipeline(client, ctx, name).ok_or("a pipeline of the library");
        let blur = |names: [&str; 4]| -> Result<[Placed; 4], &'static str> {
            let [a, b, c, d] = names.map(get);
            Ok([a?, b?, c?, d?])
        };
        Ok(Self {
            opaque: get("layer-opaque")?,
            straight: get("layer-straight")?,
            rounded: get("layer-rounded")?,
            shadow: get("shadow")?,
            across: blur(ACROSS)?,
            down: blur(DOWN)?,
        })
    }

    pub(super) fn get(&self, p: Pipe) -> Option<&Placed> {
        let blur = |r: u32| BLUR_RADII.iter().position(|&x| x == r);
        match p {
            Pipe::Opaque => Some(&self.opaque),
            Pipe::Straight => Some(&self.straight),
            Pipe::Rounded => Some(&self.rounded),
            Pipe::Shadow => Some(&self.shadow),
            Pipe::BlurH(r) => blur(r).map(|i| &self.across[i]),
            Pipe::BlurV(r) => blur(r).map(|i| &self.down[i]),
        }
    }
}

/// A frame's window of numbers.
#[derive(Default)]
struct Stats {
    frames: u32,
    us: u64,
    max_us: u64,
    clean_us: u64,
    passes: u64,
    draws: u64,
    glass: [u64; 3],
    /// Pixels of the slots' passes (what the frames redrew, RFC-0101 §8).
    area_px: u64,
    /// Where a frame's time goes: the plan, the passes' emit, their submit, the waits, the
    /// retire and the firmware served (ns).
    split: [u64; 5],
    /// The layer indices whose glass was blurred again in the window (a mask).
    glass_idx: u32,
    /// RFC-0108: the frames past the panel's period in this window, and the worst.
    ux: nexus_ux_budgets::FrameWindow,
    /// TASK-0335 P0: the textured pixels the passes drew, by kind (`desktop_work`).
    work: super::desktop_work::Work,
}

/// The buffers a desktop takes: the layout adopted, the scratches, the cache, the frame's own.
pub(crate) type Taken = [Option<BoId>; 5];

/// `taken`'s buffers given back, once the context that bound them is gone (a buffer still bound
/// is refused, never freed).
pub(crate) fn give_back(dev: &mut Device, client: ClientId, taken: &mut Taken) {
    for bo in taken.iter_mut().filter_map(Option::take) {
        let _ = dev.destroy_bo(client, bo);
    }
}

/// The frame executor of windowd's context.
pub(crate) struct Desktop {
    client: ClientId,
    ctx: ContextId,
    streams: RenderStreams<'static>,
    captured: Captured,
    pub(super) display: (u32, u32),
    layout_rows: u32,
    bos: Taken,
    slots: [Target; 3],
    scratch: Target,
    half: Target,
    cache: Target,
    pipes: Pipes,
    rings: Rings,
    pub(crate) frame: Frame,
    /// The frames' damage (RFC-0101 §8, TASK-0215 5c-3 P5) and the frame each slot was last
    /// drawn at (0: never): a slot is brought up to date by the frames it missed.
    history: History,
    pub(super) drawn: [u64; 3],
    /// The atlas rows each layer step reads that were cleaned for the device in this content
    /// generation (`CleanedRows`: a scroll cleans the rows it shows new).
    pub(super) cleaned: super::desktop_pass::CleanedRows,
    /// The base plane cleaned for the device, by its first row.
    base: Option<u32>,
    stats: Stats,
    shown: bool,
    /// The gate scene drawn (once, before the compositor's first frame).
    gated: bool,
    /// The last pass's wait (µs), the wait's iterations and the interrupts it saw.
    pub(super) pass_witness: [u64; 3],
    /// The glasses blurred again named once (`gpud: gpu glass written (`): the first window
    /// whose frames wrote glass says which layers.
    glass_named: bool,
    /// What the open cost (the bindings walk every page of the layout), for the first line.
    open_us: u64,
}

/// `len` bytes of `bo` from `offset` bound at `va`; a refusal names `what` and the device's word.
fn bind(
    dev: &mut Device,
    (client, ctx): (ClientId, ContextId),
    (bo, offset, len): (BoId, u64, u64),
    (va, access): (u64, Access),
    what: &'static str,
) -> Result<(), Refusal> {
    dev.vm_bind(client, ctx, Mapping { bo, offset, len, va: GpuVa(va), access })
        .map_err(|e| Refusal(what, Some(e)))
}

/// A buffer of `bytes` in `mode`, recorded in `taken` as soon as it exists, bound read-write at
/// `va`.
fn buffer(
    dev: &mut Device,
    who: (ClientId, ContextId),
    (bytes, mode): (u64, CacheMode),
    va: u64,
    (taken, what): (&mut Option<BoId>, &'static str),
) -> Result<(), Refusal> {
    let bo = dev.create_bo(who.0, bytes as usize, mode).map_err(|e| Refusal(what, Some(e)))?;
    *taken = Some(bo);
    bind(dev, who, (bo, 0, bytes), (va, Access::ReadWrite), what)
}

impl Desktop {
    /// What windowd's context takes for its frames, once: the layout at `layout_bus` of
    /// `layout_rows` rows, the scratches, the cache, the frame's buffer, the targets, the
    /// pipelines. Every buffer is recorded in `taken` as it is made: on a refusal the caller
    /// gives them back once the context is gone; an open desktop holds them itself.
    pub(crate) fn open(
        dev: &mut Device,
        (client, ctx): (ClientId, ContextId),
        (layout_bus, layout_rows): (u64, u32),
        display: (u32, u32),
        taken: &mut Taken,
    ) -> Result<Self, Refusal> {
        let t0 = now_ns();
        if display != (WIDTH, PLANE_ROWS) {
            return Err("a display other than the render bundle's".into());
        }
        let who = (client, ctx);
        let scene = Scene::parse(RENDER_BUNDLE, CoreId(dev.gpu.win.core_id()))
            .map_err(|_| "the render bundle")?;
        let streams = render_streams(&scene).ok_or("the render bundle's streams")?;
        let captured = Captured::read(dev, client, ctx)?;
        // The layout in three ranges at its own offsets: the wallpaper and retained planes, the
        // ring's slots (the targets), the atlas.
        let layout_len = u64::from(layout_rows) * STRIDE;
        let ring_at = layout::row_offset_bytes(RING_ROWS[0]) as u64;
        let ring_end = ring_at + 3 * PLANE_BYTES as u64;
        if layout_len <= ring_end {
            return Err("a layout without its atlas".into());
        }
        let layout = dev
            .adopt_plane(client, layout_bus, layout_len as usize)
            .map_err(|e| Refusal("the layout", Some(e)))?;
        taken[0] = Some(layout);
        let read = |at: u64| (LAYOUT_VA + at, Access::ReadOnly);
        bind(dev, who, (layout, 0, ring_at), read(0), "the planes' binding")?;
        let ring = (layout, ring_at, ring_end - ring_at);
        bind(dev, who, ring, (LAYOUT_VA + ring_at, Access::ReadWrite), "the ring's binding")?;
        let atlas = (layout, ring_end, layout_len - ring_end);
        bind(dev, who, atlas, read(ring_end), "the atlas's binding")?;
        let plane = |rows: u32| (u64::from(rows) * STRIDE, CacheMode::GpuPrivate);
        let [_, scratch, half, cache, frame_bo] = &mut *taken;
        buffer(dev, who, plane(PLANE_ROWS), SCRATCH_VA, (scratch, "the scratch"))?;
        buffer(dev, who, plane(HALF_ROWS), HALF_VA, (half, "the half scratch"))?;
        buffer(dev, who, plane(CACHE_ROWS), CACHE_VA, (cache, "the glass cache"))?;
        let frame_len = (u64::from(FRAME_BYTES), CacheMode::CpuCached);
        buffer(dev, who, frame_len, FRAME_VA, (frame_bo, "the pass memory"))?;
        let mut targets = Targets::new();
        let mut at = |dev: &mut Device, va: u64, rows: u32| {
            targets.at(dev, who, &captured, va, rows).map_err(|_| "a target")
        };
        let slot_va = |k: u64| LAYOUT_VA + ring_at + k * PLANE_BYTES as u64;
        let slots = [
            at(dev, slot_va(0), PLANE_ROWS)?,
            at(dev, slot_va(1), PLANE_ROWS)?,
            at(dev, slot_va(2), PLANE_ROWS)?,
        ];
        let scratch_t = at(dev, SCRATCH_VA, PLANE_ROWS)?;
        let half_t = at(dev, HALF_VA, HALF_ROWS)?;
        let cache_t = at(dev, CACHE_VA, CACHE_ROWS)?;
        let pipes = Pipes::named(dev, client, ctx)?;
        let rings = Rings::new(
            Arena { base: 0, len: GENERAL_BYTES },
            Arena { base: TARGET_PDS.1, len: PDS_END - TARGET_PDS.1 },
            Arena { base: GENERAL_BYTES, len: STREAM_BYTES },
            Arena { base: GENERAL_BYTES + STREAM_BYTES, len: VERTEX_BYTES },
        );
        let mut frame = Frame::new(CACHE_ROWS);
        frame.glass.ready = true;
        Ok(Self {
            client,
            ctx,
            streams,
            captured,
            display,
            layout_rows,
            bos: core::mem::take(taken),
            slots,
            scratch: scratch_t,
            half: half_t,
            cache: cache_t,
            pipes,
            rings,
            frame,
            history: History::new(),
            drawn: [0; 3],
            cleaned: super::desktop_pass::CleanedRows::new(),
            base: None,
            stats: Stats::default(),
            shown: false,
            gated: false,
            pass_witness: [0; 3],
            glass_named: false,
            open_us: now_ns().saturating_sub(t0) / 1000,
        })
    }

    /// The buffers given back, after the context that bound them is gone.
    pub(crate) fn release(mut self, dev: &mut Device) {
        give_back(dev, self.client, &mut self.bos);
    }

    /// One frame of windowd's commands into the slot `target` names: planned, its textures
    /// cleaned where the CPU wrote them, its passes drawn and waited. The gate scene first,
    /// once (5c-4).
    pub(crate) fn draw(
        &mut self,
        dev: &mut Device,
        cmds: &[Command],
        target: &FrameTarget<'_>,
    ) -> Result<nexus_gfx::backend::types::Rect, Refusal> {
        if !self.gated {
            self.gated = true;
            self.gate(dev, target)?;
        }
        // The frame's passes pipelined: one wait at the end (cycle 39's pass latency).
        self.frame_of(dev, cmds, target, (false, false, true), target.base_dirty)
            .map(|(rect, _, _)| rect)
    }

    /// A frame of `cmds`: the rectangle its slot pass redrew, its time and its passes; `quiet`
    /// leaves the statistics and the lines alone (the gate scene).
    pub(super) fn frame_of(
        &mut self,
        dev: &mut Device,
        cmds: &[Command],
        target: &FrameTarget<'_>,
        (quiet, trace, pipelined): (bool, bool, bool),
        base_dirty: bool,
    ) -> Result<(nexus_gfx::backend::types::Rect, u64, usize), Refusal> {
        let t0 = now_ns();
        let k = RING_ROWS
            .iter()
            .position(|&r| r == target.slot_row)
            .ok_or("a slot outside the ring")?;
        let slot = self.slots[k];
        self.frame.layers.take_frame(cmds);
        let c0 = now_ns();
        // The base the scene starts from: cleaned when it is new or the CPU rewrote it — and
        // every glass's backdrop over the old one is stale, every slot too.
        let mut lacks = Area::None;
        if self.base != Some(target.base_row) || base_dirty {
            clean_rows(target, (0, target.base_row, WIDTH, PLANE_ROWS));
            self.frame.glass.wallpaper_epoch = self.frame.glass.wallpaper_epoch.wrapping_add(1);
            self.base = Some(target.base_row);
            self.drawn = [0; 3];
        }
        let p0 = now_ns();
        let planned = self.frame.plan(self.display, &|_| true);
        let plan_ns = now_ns().saturating_sub(p0);
        let mut glass_idx = 0u32;
        for i in 0..planned.layers.min(32) {
            let written = self.frame.step(i).and_then(|s| s.glass).map(|g| g.cache);
            if matches!(written, Some(CacheStep::Write(_))) {
                glass_idx |= 1 << i;
            }
        }
        // The glasses blurred again, named once past the first frames (the settled desktop's
        // writes are the question, cycles 35–37: `glass_idx=0x6` every frame while typing).
        if !quiet && glass_idx != 0 && !self.glass_named && self.stats.frames >= 8 {
            self.glass_named = true;
            // The present's damage this frame, with its origins: what touched them.
            let (named, count, full) = self.frame.layers.last_damage();
            let mut line = alloc::string::String::new();
            for (r, o) in named.iter().take(count) {
                use core::fmt::Write as _;
                if let Area::Rect { x0, y0, x1, y1 } = r {
                    let _ = write!(line, " {x0},{y0},{}x{}@{o}", x1 - x0, y1 - y0);
                }
            }
            emit_fmt(format_args!("gpud: gpu glass damage (full={full} rects={line})"));
            for i in 0..planned.layers.min(32) {
                if glass_idx & (1 << i) == 0 {
                    continue;
                }
                if let Some(l) = self.frame.step(i).map(|s| s.layer) {
                    emit_fmt(format_args!(
                        "gpud: gpu glass written (i={i} rect={},{},{}x{} id={} blur={} src_row={})",
                        l.dst_x,
                        l.dst_y,
                        l.width,
                        l.height,
                        l.layer_id,
                        l.backdrop_blur,
                        l.src_row_abs
                    ));
                }
            }
        }
        // What the slot lacks: the frames since it was last drawn (everything when the CPU
        // drew it since, or it never was) and this frame's damage.
        if !target.slot_stale {
            lacks = lacks.union(self.history.since(self.drawn[k]));
        } else {
            lacks = Area::Full;
        }
        self.drawn[k] = self.history.record(planned.damage);
        let area = lacks.union(planned.damage).on(self.display);
        // The atlas rows a layer reads, cleaned when they are new to the device — a content
        // generation, or rows a moved source (a scroll) shows for the first time: rows the CPU
        // wrote and never cleaned read stale.
        if planned.upload {
            self.cleaned.forget();
        }
        for i in 0..planned.layers {
            if let Some(s) = self.frame.step(i) {
                let l = s.layer;
                let (w, h) = (
                    if l.content_w > 0 { l.content_w } else { l.width },
                    if l.content_h > 0 { l.content_h } else { l.height },
                );
                self.cleaned.clean(target, i, (l.src_x, l.src_row_abs, w, h));
            }
        }
        let clean_us = now_ns().saturating_sub(c0) / 1000;
        let (display, layout_rows, who) = (self.display, self.layout_rows, (self.client, self.ctx));
        let (scratch, half, cache) = (self.scratch, self.half, self.cache);
        let Self { frame, rings, pipes, captured, streams, .. } = self;
        let board = Board::new(frame, planned, display, layout_rows, HALF_ROWS)
            .with_base(target.base_row)
            .with_area(area.unwrap_or((0, 0, 0, 0)));
        // Nothing of the slot lacks: no pass at all — it shows the scene as it is.
        let passes = if area.is_some() { board.pass_count() } else { 0 };
        let t = captured.template(FRAME_VA, FRAME_VA, FRAME_VA);
        let mut draws = 0;
        let mut split = [plan_ns, 0, 0, 0, 0];
        let mut witness = [0u64; 3];
        let mut work = super::desktop_work::Work::default();
        work.note_writes(planned.writes);
        // At most two passes in flight: the context's two render-target data alternate per
        // pass (the firmware's, `render.rs`); a third pass in the queue would reuse the data of
        // one still drawing — cycle 40's lockup. Pass p waits for pass p − 2 before it is
        // submitted.
        let mut last: Option<nexus_driverkit::gpu::TimelinePoint> = None;
        let mut points: [Option<nexus_driverkit::gpu::TimelinePoint>; 2] = [None; 2];
        for p in 0..passes {
            let spec = board.pass(p).ok_or("a pass of the plan")?;
            let to = match spec.target {
                To::Slot => slot,
                To::Scratch => scratch,
                To::Half => half,
                To::Cache => cache,
            };
            work.note(spec.target, board.work_by_program(&spec));
            let wait = !pipelined || p + 1 == passes;
            if let Some(two_back) = points[p % 2] {
                let q0 = now_ns();
                super::desktop_pass::wait_point(dev, who, two_back)?;
                split[3] += now_ns().saturating_sub(q0);
            }
            let (n, stamps, point) = run_pass(
                dev,
                who,
                streams,
                &t,
                pipes,
                rings,
                &board,
                &spec,
                to,
                (trace && wait, wait),
                last,
            )?;
            last = Some(point);
            points[p % 2] = Some(point);
            draws += n;
            for (acc, ns) in split[1..].iter_mut().zip(stamps) {
                *acc += ns;
            }
            witness = [stamps[2] / 1000, stamps[4], stamps[5]];
        }
        self.pass_witness = witness;
        let us = now_ns().saturating_sub(t0) / 1000;
        let glass = board.glass;
        let area_px =
            area.map_or(0, |(x0, y0, x1, y1)| u64::from(x1 - x0 + 1) * u64::from(y1 - y0 + 1));
        if !quiet {
            let s = &mut self.stats;
            s.frames += 1;
            s.us += us;
            s.max_us = s.max_us.max(us);
            s.ux.note(us);
            s.work.add(work);
            s.clean_us += clean_us;
            s.passes += passes as u64;
            s.draws += draws as u64;
            s.area_px += area_px;
            s.glass_idx |= glass_idx;
            for (acc, ns) in s.split.iter_mut().zip(split) {
                *acc += ns;
            }
            for (acc, n) in s.glass.iter_mut().zip([glass.writes, glass.reads, glass.off]) {
                *acc += u64::from(n);
            }
            if !self.shown {
                self.shown = true;
                emit_fmt(format_args!(
                    "gpud: gpu desktop ok (frame_us={us} passes={passes} draws={draws} layers={} glass_writes={} glass_reads={} glass_off={} area_px={area_px} clean_us={clean_us} open_us={})",
                    planned.layers,
                    glass.writes,
                    glass.reads,
                    glass.off,
                    self.open_us
                ));
            }
            if s.frames == WINDOW {
                let n = u64::from(WINDOW);
                emit_fmt(format_args!(
                    "gpud: gpu frame us (n={n} avg={} max={} clean={} passes={}.{} draws={} area_px={} glass_w={} glass_r={} glass_off={} glass_idx={:#x} split=plan {}/emit {}/submit {}/wait {}/retire {})",
                    s.us / n,
                    s.max_us,
                    s.clean_us / n,
                    s.passes / n,
                    (s.passes % n) * 10 / n,
                    s.draws / n,
                    s.area_px / n,
                    s.glass[0],
                    s.glass[1],
                    s.glass[2],
                    s.glass_idx,
                    s.split[0] / n / 1000,
                    s.split[1] / n / 1000,
                    s.split[2] / n / 1000,
                    s.split[3] / n / 1000,
                    s.split[4] / n / 1000
                ));
                // RFC-0108: the window judged — frames past 16.7 ms are dropped frames.
                emit_fmt(format_args!(
                    "gpud: ux frame {} (dropped={} of={} max_us={} budget_us={})",
                    s.ux.verdict().as_str(),
                    s.ux.dropped,
                    s.ux.frames,
                    s.ux.max_us,
                    nexus_ux_budgets::FRAME_US
                ));
                // TASK-0335 P0: the textured pixels per frame by pass kind and the GPU's time
                // per pixel — the cost model every cut of this package is judged by.
                s.work.emit(n, s.split[3]);
                *s = Stats::default();
            }
        }
        let rect =
            area.map_or(nexus_gfx::backend::types::Rect { x: 0, y: 0, width: 0, height: 0 }, |a| {
                nexus_gfx::backend::types::Rect {
                    x: a.0,
                    y: a.1,
                    width: a.2 - a.0 + 1,
                    height: a.3 - a.1 + 1,
                }
            });
        Ok((rect, us, passes))
    }
}
