// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A desktop frame's passes on the device (TASK-0215 5c-3 P4b/P5, split out of `desktop.rs`
//! under the module-size ratchet): the layout's lines cleaned for the device (a scroll's new rows
//! alone, TASK-0335 P4), the frame's pass memory as the emitter's sink, a texture's descriptor in
//! fresh lines, and one pass — its descriptors, the draws that touch its area cut to it
//! (`nexus_gfx::frame::clip_draw`, TASK-0351 P5: the area reaches the hardware as the quads,
//! not only as the back end's clip) and emitted, submitted and waited, stamped at each step for
//! the frame's time split.

use alloc::vec::Vec;
use nexus_display_proto::layout::{self, STRIDE_BYTES};
use nexus_driverkit::gpu::{GpuVa, QueueKind, TimelinePoint};
use nexus_gfx::backend::gpu::pipelines::Placed;
use nexus_gfx::backend::gpu::quads::{
    emit_draws, Draw, EmitError, Need, Placer, Quad, Region, Rings, Sink, Template,
};
use nexus_gfx::backend::gpu::texstate::{descriptor_bytes, format, swizzle, Image, Sampler};
use nexus_gfx::backend::v2::{
    Batch, ClientId, ContextId, GfxDevice, RenderStreams, RenderTarget, Stream,
};
use nexus_gfx::frame::{clip_draw, Board, DrawSpec, PassSpec, Pipe, Tex, BLUR_RADII};

use super::desktop::{
    Refusal, FRAME_VA, HALF_VA, LAYOUT_VA, MAX_DRAWS, PASS_TIMEOUT_NS, SCRATCH_VA,
};
use super::device_v2::Device;
use super::emit_fmt;
use super::render_cost::streams_for;
use super::render_template::{pds_at, write_bound, Target, PDS_ARENA};
use super::surface_buffers::SurfaceBuffers;
use crate::gpu_context::FrameTarget;

const WIDTH: u32 = STRIDE_BYTES / 4;
const STRIDE: u64 = STRIDE_BYTES as u64;
/// A single-pass frame waiting longer than this arms the next frame's pass trace.
const SLOW_WAIT_US: u64 = 20_000;
/// Slow single-pass frames traced in a 32-frame window, at most.
const SLOW_TRACES: u8 = 4;
/// Frames in the window the traces are counted over.
const SLOW_WINDOW: u8 = 32;

/// The blur's names in the library, by [`BLUR_RADII`]'s order: the paired program (a frame's
/// passes both read a source at the blur's resolution; the step says across or down).
const BLUR: [&str; 4] =
    ["blur-vertical-8", "blur-vertical-20", "blur-vertical-30", "blur-vertical-40"];

/// The library's pipelines a frame draws with.
pub(super) struct Pipes {
    opaque: Placed,
    straight: Placed,
    rounded: Placed,
    /// A run drawn back (RFC-0109 "Runs and group opacity"): its texels hold colour times
    /// coverage, so the group fades as one — `desktop·(1 − a) + group·a`.
    premultiplied: Placed,
    shadow: Placed,
    blur: [Placed; 4],
}

impl Pipes {
    pub(super) fn named(
        dev: &Device,
        client: ClientId,
        ctx: ContextId,
    ) -> Result<Self, &'static str> {
        let get = |name: &str| dev.pipeline(client, ctx, name).ok_or("a pipeline of the library");
        let [a, b, c, d] = BLUR.map(get);
        Ok(Self {
            opaque: get("layer-opaque")?,
            straight: get("layer-straight")?,
            rounded: get("layer-rounded")?,
            premultiplied: get("layer-premultiplied")?,
            shadow: get("shadow")?,
            blur: [a?, b?, c?, d?],
        })
    }

    fn get(&self, p: Pipe) -> Option<&Placed> {
        let blur = |r: u32| BLUR_RADII.iter().position(|&x| x == r);
        match p {
            Pipe::Opaque => Some(&self.opaque),
            Pipe::Straight => Some(&self.straight),
            Pipe::Rounded => Some(&self.rounded),
            Pipe::Premultiplied => Some(&self.premultiplied),
            Pipe::Shadow => Some(&self.shadow),
            Pipe::Blur(r) => blur(r).map(|i| &self.blur[i]),
        }
    }
}

/// The trace of the single-pass frames stuck at ~21 or ~41 ms (TASK-0373, 21 % of the heavy
/// slow frames on board--2026-11-19T22-09-42 and …2026-11-20T08-55-45, no glass write): one
/// waiting past [`SLOW_WAIT_US`] arms the next frame's `gpu pass fw`/`gpu pass trace` lines —
/// the runs are 3-6 frames long — so the firmware's clock says whether the GPU was busy or the
/// completion's wake came late. At most [`SLOW_TRACES`] a window; the gate and sweep frames
/// (`quiet`) never arm it.
pub(super) struct SlowTrace {
    armed: bool,
    left: u8,
    frames: u8,
}

impl SlowTrace {
    pub(super) const fn new() -> Self {
        Self { armed: false, left: SLOW_TRACES, frames: 0 }
    }

    /// Whether this frame's pass is traced — the arm spent, the lines named `single`.
    pub(super) fn take(&mut self, trace_of: &mut &'static str) -> bool {
        let armed = core::mem::take(&mut self.armed);
        if armed {
            *trace_of = "single";
        }
        armed
    }

    /// A frame's passes and its last pass's wait (µs): a slow single pass arms the next frame.
    pub(super) fn note(&mut self, quiet: bool, passes: usize, wait_us: u64) {
        if quiet {
            return;
        }
        self.frames += 1;
        if self.frames >= SLOW_WINDOW {
            (self.frames, self.left) = (0, SLOW_TRACES);
        }
        if passes == 1 && wait_us > SLOW_WAIT_US && self.left > 0 {
            (self.armed, self.left) = (true, self.left - 1);
        }
    }
}

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// The CPU's lines of the rectangle (`x`, `row`, `w`, `h`) of the layout written back, so the
/// device reads what the CPU wrote.
pub(super) fn clean_rows(target: &FrameTarget<'_>, (x, row, w, h): (u32, u32, u32, u32)) {
    if target.block == 0 {
        return;
    }
    for r in row..row.saturating_add(h) {
        let at = layout::row_offset_bytes(r) + x as usize * 4;
        if let Some(span) = target.layout.get(at..at + w as usize * 4) {
            nexus_abi::cache_clean(span, target.block);
        }
    }
}

/// The atlas rows each layer step reads, as far as they were cleaned for the device in the
/// current content generation: columns `x`, width `w`, rows `lo..hi` (TASK-0335 P4). A scroll
/// moves a layer's source by a few rows a frame and only those are new to the device — cleaning
/// the whole source again cost a scroll frame 2.5 ms of CPU (cycle 61). A new generation (the
/// CPU wrote rows: the plan's `upload`, windowd's content epoch moved) forgets every range; a
/// source in other columns or apart from its cleaned rows is cleaned whole.
pub(super) struct CleanedRows(Vec<Option<(u32, u32, u32, u32)>>);

impl CleanedRows {
    pub(super) const fn new() -> Self {
        Self(Vec::new())
    }

    /// A new content generation: no row counts as cleaned.
    pub(super) fn forget(&mut self) {
        self.0.clear();
    }

    /// Cleans what layer step `i`'s source `(x, row, w, h)` holds beyond the rows cleaned since
    /// the generation began.
    pub(super) fn clean(
        &mut self,
        target: &FrameTarget<'_>,
        i: usize,
        (x, row, w, h): (u32, u32, u32, u32),
    ) {
        let end = row.saturating_add(h);
        // The table grows with the plan's steps (a heap that refuses cleans the source whole).
        if i >= self.0.len()
            && i < nexus_gfx::frame::MAX_LAYERS
            && self.0.try_reserve(i + 1 - self.0.len()).is_ok()
        {
            self.0.resize(i + 1, None);
        }
        let Some(slot) = self.0.get_mut(i) else {
            clean_rows(target, (x, row, w, h));
            return;
        };
        match *slot {
            Some((cx, cw, lo, hi)) if (cx, cw) == (x, w) && row <= hi && end >= lo => {
                if row < lo {
                    clean_rows(target, (x, row, w, lo - row));
                }
                if end > hi {
                    clean_rows(target, (x, hi, w, end - hi));
                }
                *slot = Some((x, w, lo.min(row), hi.max(end)));
            }
            _ => {
                clean_rows(target, (x, row, w, h));
                *slot = Some((x, w, row, end));
            }
        }
    }
}

/// The frame's pass memory: the PDS heap's lines in the bundle's PDS arena, everything else in
/// the frame's buffer.
struct FrameSink<'a> {
    dev: &'a mut Device,
    who: (ClientId, ContextId),
}

impl Sink for FrameSink<'_> {
    fn write(&mut self, region: Region, offset: u32, bytes: &[u8]) -> bool {
        let (va, at) = match region {
            Region::Pds => (PDS_ARENA, pds_at(PDS_ARENA, offset)),
            Region::General | Region::Stream | Region::Vertex => (FRAME_VA, offset),
        };
        UPLOADED.fetch_add(bytes.len() as u64, core::sync::atomic::Ordering::Relaxed);
        write_bound(self.dev, self.who.0, self.who.1, va, at, bytes).is_ok()
    }
}

/// Bytes written into the frame's pass memory — its streams, vertices, PDS lines and descriptors
/// (TASK-0373: the frame's `cp=` on `gpud: hops one`). Relaxed: a count, never a synchronisation.
static UPLOADED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// The bytes written into the pass memory since the last call, and a fresh count.
pub(super) fn take_uploaded() -> u64 {
    UPLOADED.swap(0, core::sync::atomic::Ordering::Relaxed)
}

/// `tex`'s descriptor written into fresh lines of the frame's buffer: its address (`none`, the
/// bundle's own, for a draw that samples nothing).
fn descriptor(
    dev: &mut Device,
    who: (ClientId, ContextId),
    rings: &mut Rings,
    (tex, buffers): (Tex, &SurfaceBuffers),
    none: u64,
) -> Result<u64, &'static str> {
    let at = |va: u64, x: u32, row: u32| va + u64::from(row) * STRIDE + u64::from(x) * 4;
    let (address, width, height, stride) = match tex {
        Tex::None => return Ok(none),
        Tex::Layout { x, row, w, h } => (at(LAYOUT_VA, x, row), w, h, WIDTH),
        // The scene below a glass: its own rectangle, so a tap past the display's edge clamps
        // at the scene's edge texel (TASK-0351 P19).
        Tex::Scratch { x, row, w, h } => (at(SCRATCH_VA, x, row), w, h, WIDTH),
        Tex::Half { rows } => (HALF_VA, WIDTH, rows, WIDTH),
        Tex::Cache { x, row, w, h } => {
            let va = super::desktop_cache::texel_va(x, row).ok_or("a cache row past the planes")?;
            (va, w, h, WIDTH)
        }
        // A run's plane (RFC-0101 §8): the planner's own texture, never a command's source.
        Tex::Run { slot, x, row, w, h } => {
            let va = crate::surface_va::run_plane_va(u32::from(slot)).ok_or("a run plane")?;
            (at(va, x, row), w, h, WIDTH)
        }
        // RFC-0114: an app's buffer at its own address and stride, where the app drew it.
        Tex::Buffer { source, slot, x, row, w, h } => {
            let (address, stride) =
                buffers.texels(source, slot, (x, row, w, h)).ok_or("a buffer the context lacks")?;
            (address, w, h, stride)
        }
    };
    let image =
        Image { address, width, height, stride, format: format::U8U8U8U8, swizzle: swizzle::BGRA };
    let bytes = descriptor_bytes(&image, &Sampler::BILINEAR_CLAMP).map_err(|_| "a descriptor")?;
    let off = rings.place(0, Need::Descriptor).ok_or("no line for a descriptor")?;
    UPLOADED.fetch_add(bytes.len() as u64, core::sync::atomic::Ordering::Relaxed);
    write_bound(dev, who.0, who.1, FRAME_VA, off, &bytes).map_err(|_| "a descriptor's write")?;
    Ok(FRAME_VA + u64::from(off))
}

/// The context's fragment timeline reached `point` (the pass it names is drawn), waited.
pub(super) fn wait_point(
    dev: &mut Device,
    who: (ClientId, ContextId),
    point: TimelinePoint,
) -> Result<(), Refusal> {
    let Some((_, (ftl, _))) = dev.render_progress(who.0, who.1) else {
        return Err("the context's render objects".into());
    };
    let seq = point.0 as u32;
    let done = dev.wait_until(now_ns() + PASS_TIMEOUT_NS, |g, _| {
        g.mem.read_u32(&ftl, 0).wrapping_sub(seq) < 0x8000_0000
    });
    dev.serve_firmware();
    if done {
        Ok(())
    } else {
        Err("a pass not reached in time".into())
    }
}

/// One pass of the frame: its descriptors, its draws emitted, submitted — and waited when
/// `wait` (the frame's last pass; the passes before it are pipelined: the firmware runs a
/// context's passes in order, and a pass's fragment stage came 12 ms after its geometry on
/// the board — cycle 39 — a latency hidden only by the next pass being in the queue). When the
/// rings hold no line for the pass, the pass `last` submitted is waited for and the emit tried
/// once more. The number of draws emitted (those touching the pass's area), the stamps, the
/// pass's point.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_pass(
    dev: &mut Device,
    who: (ClientId, ContextId),
    base: &RenderStreams<'_>,
    t: &Template<'_>,
    pipes: &Pipes,
    rings: &mut Rings,
    kept: &mut Kept,
    board: &Board<'_>,
    spec: &PassSpec,
    (target, buffers): (Target, &SurfaceBuffers),
    (trace, wait): (Option<&'static str>, bool),
    last: Option<TimelinePoint>,
) -> Result<(usize, [u64; 6], TimelinePoint), Refusal> {
    for attempt in 0..2 {
        rings.begin();
        let ran = emit_and_wait(
            dev,
            who,
            base,
            t,
            pipes,
            (rings, kept),
            board,
            spec,
            (target, buffers),
            trace,
            wait,
        );
        match ran {
            Ok((point, mut stamps, kept)) => {
                let r0 = now_ns();
                rings.submitted(point.0).map_err(|_| "the rings")?;
                rings.retire(&|p| dev.reached(TimelinePoint(p)));
                stamps[3] = now_ns().saturating_sub(r0);
                return Ok((kept, stamps, point));
            }
            Err(Refusal(NO_ROOM, None)) if attempt == 0 && last.is_some() => {
                rings.abandon();
                if let Some(p) = last {
                    wait_point(dev, who, p)?;
                }
                rings.retire(&|p| dev.reached(TimelinePoint(p)));
            }
            Err(e) => {
                rings.abandon();
                return Err(e);
            }
        }
    }
    Err(NO_ROOM.into())
}

/// The emit's refusal when the rings hold no line (retried once after the last pass).
const NO_ROOM: &str = "the pass's emit: no room in the pass memory";
/// A pass the plan gives no draw for.
const NO_DRAWS: &str = "a pass of no draws";
/// Passes that left their lowest draws out (`frame_limits::left_out`); named at 1, 2, 4, ….
static OVER: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// The draws a pass keeps (TASK-0351 P10): walked once from the plan (`Board::walk`), each cut
/// to the pass's area, with its texture's descriptor — the emitter indexes them instead of
/// asking the plan per draw, twice. Allocated once for `MAX_DRAWS`, reused by every pass.
pub(super) struct Kept {
    draws: Vec<DrawSpec>,
    tex: Vec<u64>,
}

impl Kept {
    pub(super) fn new() -> Self {
        Self { draws: Vec::with_capacity(MAX_DRAWS), tex: Vec::with_capacity(MAX_DRAWS) }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_and_wait(
    dev: &mut Device,
    who: (ClientId, ContextId),
    base: &RenderStreams<'_>,
    t: &Template<'_>,
    pipes: &Pipes,
    (rings, kept): (&mut Rings, &mut Kept),
    board: &Board<'_>,
    spec: &PassSpec,
    (target, buffers): (Target, &SurfaceBuffers),
    trace: Option<&'static str>,
    wait: bool,
) -> Result<(TimelinePoint, [u64; 6], usize), Refusal> {
    let e0 = now_ns();
    // The draws that touch the pass's area, cut to it — one outside it writes no pixel (TASK-0335
    // P4: a frame inside a window skips the shadows, the corners and the layers elsewhere; the
    // last draw stands in when none does), one across it is shaded only where the area is
    // (TASK-0351 P5) — walked once from the plan with their descriptors (TASK-0351 P10).
    kept.draws.clear();
    kept.tex.clear();
    let mut last = None;
    for whole in board.walk(spec) {
        last = Some(whole);
        kept.draws.extend(clip_draw(&whole, spec.area));
    }
    if kept.draws.is_empty() {
        kept.draws.push(last.ok_or(NO_DRAWS)?);
    }
    // A pass of more draws than its memory holds draws its topmost — what lies lowest is what
    // the layers above cover most. Named, never a refused frame (TASK-0351 P6).
    let over = crate::frame_limits::left_out(kept.draws.len());
    if over > 0 {
        kept.draws.drain(..over);
        let n = OVER.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
        if n.is_power_of_two() {
            emit_fmt(format_args!(
                "gpud: gpu pass draws FAIL (kept={MAX_DRAWS} left_out={over} passes={n}: the lowest draws are not drawn)"
            ));
        }
    }
    // Each kept draw's pipeline checked, its texture's descriptor written.
    for d in &kept.draws {
        pipes.get(d.pipe).ok_or("a pipeline the library lacks")?;
        kept.tex.push(descriptor(dev, who, rings, (d.tex, buffers), t.texture_va)?);
    }
    let m = kept.draws.len();
    // Every draw was checked above; an empty quad keeps the closure total.
    let draw = |j: usize| {
        let d = kept.draws.get(j);
        let pipeline = d.and_then(|d| pipes.get(d.pipe)).unwrap_or(&pipes.opaque);
        let quad = d.map_or(Quad::layer(0.0, 0.0, 0.0, 0.0, [0.0; 4], 0.0), |d| d.quad);
        Draw { pipeline, quad, texture_va: kept.tex.get(j).copied().unwrap_or(0) }
    };
    let mut sink = FrameSink { dev: &mut *dev, who };
    let emitted = emit_draws(t, m, &draw, rings, &mut sink).map_err(|e| match e {
        EmitError::Space => NO_ROOM,
        EmitError::Sink => "the pass's emit: a write refused",
        EmitError::Pipeline => "the pass's emit: a pipeline's setup",
        EmitError::Template | EmitError::NoQuads => "the pass's emit: the template",
    })?;
    let feats = dev.gpu.fw.render;
    let (geom, frag_pr, frag) =
        streams_for(base, feats, emitted.vdm_ctrl_stream_base, Some(spec.area), target)?;
    let batch = Batch {
        target: RenderTarget { va: GpuVa(target.va), width: WIDTH, height: target.rows },
        stream: Stream::Render(RenderStreams {
            geom: &geom,
            frag_pr: &frag_pr,
            frag: &frag,
            ..*base
        }),
        waits: &[],
        signal: TimelinePoint(0),
    };
    // The firmware's view before the kick (TASK-0351 P12), for the traced pass's fw line.
    let fw0 = trace.map(|_| dev.gpu.fw.view(&dev.gpu.mem));
    let s0 = now_ns();
    let point = dev
        .submit(who.0, who.1, QueueKind::Render, &batch)
        .map_err(|e| Refusal("the submit", Some(e)))?;
    let Some((_, (ftl, fseq))) = dev.render_progress(who.0, who.1) else {
        return Err("the context's render objects".into());
    };
    if !wait {
        // Pipelined: the next pass follows in the queue; the frame's last pass waits for all.
        let w0 = now_ns();
        return Ok((point, [s0 - e0, w0 - s0, 0, 0, 0, 0], m));
    }
    let w0 = now_ns();
    let irq0 = dev.gpu.irqs;
    let mut iters = 0u64;
    // The trace: the wait's polls as their changes and its end — the time since the submit, the
    // interrupts seen by then, and whether the geometry and the fragment words were reached.
    let mut steps = crate::backend::pass_trace::PassTrace::new();
    let (gtl, gseq) = dev.render_progress(who.0, who.1).map(|(g, _)| g).unwrap_or((ftl, fseq));
    let done = dev.wait_until(w0 + PASS_TIMEOUT_NS, |g, _| {
        let frag = g.mem.read_u32(&ftl, 0).wrapping_sub(fseq) < 0x8000_0000;
        if trace.is_some() {
            let geom = g.mem.read_u32(&gtl, 0).wrapping_sub(gseq) < 0x8000_0000;
            let at = (now_ns().saturating_sub(w0) / 1000) as u32;
            steps.note((at, g.irqs.wrapping_sub(irq0), u8::from(geom) | (u8::from(frag) << 1)));
        }
        iters += 1;
        frag
    });
    let w1 = now_ns();
    // The most a timer-ended slice of this wait woke after its deadline: the kernel's
    // wake-to-run latency (ADR-0096), on the trace line beside the iterations.
    let late_us = dev.gpu.late_ns / 1000;
    dev.serve_firmware();
    let irqs = u64::from(dev.gpu.irqs.wrapping_sub(irq0));
    if let (Some(of), Some(fw0)) = (trace, fw0) {
        // The pass by the firmware's clock: what it executed, the interrupts it took, and the
        // time it spent idle, active and blocked between the kick and the fragment's end.
        let fw1 = dev.gpu.fw.view(&dev.gpu.mem);
        let d = |i: usize| fw1.util[i].wrapping_sub(fw0.util[i]);
        emit_fmt(format_args!(
            "gpud: gpu pass fw (of={of} wait_us={} exec=+{} irqs=+{} util_idle=+{} util_active=+{} util_blocked=+{} state={}>{})",
            (w1 - w0) / 1000,
            fw1.executed.wrapping_sub(fw0.executed),
            fw1.irqs.wrapping_sub(fw0.irqs),
            d(0),
            d(1),
            d(2),
            fw0.state,
            fw1.state
        ));
        // The pixels the pass's draws cover, overdraw counted: ns per pixel beside the wait.
        let px: u64 =
            kept.draws.iter().map(|d| (d.quad.w.max(0.0) * d.quad.h.max(0.0)) as u64).sum();
        emit_fmt(format_args!("{}", steps.line(of, irqs, late_us, px)));
    }
    let stamps = [s0 - e0, w0 - s0, w1 - w0, now_ns().saturating_sub(w1), iters, irqs];
    if done {
        Ok((point, stamps, m))
    } else {
        Err("a pass not reached in time".into())
    }
}
