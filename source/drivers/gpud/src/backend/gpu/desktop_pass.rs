// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A desktop frame's passes on the device (TASK-0215 5c-3 P4b/P5, split out of `desktop.rs`
//! under the module-size ratchet): the layout's lines cleaned for the device (a scroll's new rows
//! alone, TASK-0335 P4), the frame's pass memory as the emitter's sink, a texture's descriptor in
//! fresh lines, and one pass — its descriptors, the draws that touch its area emitted, submitted
//! and waited, stamped at each step for the frame's time split.

use nexus_display_proto::layout::{self, PLANE_ROWS, STRIDE_BYTES};
use nexus_driverkit::gpu::{GpuVa, QueueKind, TimelinePoint};
use nexus_gfx::backend::gpu::quads::{
    emit_draws, Draw, EmitError, Need, Placer, Quad, Region, Rings, Sink, Template,
};
use nexus_gfx::backend::gpu::texstate::{descriptor_bytes, format, swizzle, Image, Sampler};
use nexus_gfx::backend::v2::{
    Batch, ClientId, ContextId, GfxDevice, RenderStreams, RenderTarget, Stream,
};
use nexus_gfx::frame::{Board, PassSpec, Tex};

use super::desktop::{
    Pipes, Refusal, CACHE_VA, FRAME_VA, HALF_VA, LAYOUT_VA, MAX_DRAWS, PASS_TIMEOUT_NS, SCRATCH_VA,
};
use super::device_v2::Device;
use super::emit_fmt;
use super::render_cost::streams_for;
use super::render_template::{pds_at, write_bound, Target, PDS_ARENA};
use crate::gpu_context::FrameTarget;

const WIDTH: u32 = STRIDE_BYTES / 4;
const STRIDE: u64 = STRIDE_BYTES as u64;
/// Wait iterations a pass trace keeps.
const TRACE_STEPS: usize = 24;

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
pub(super) struct CleanedRows([Option<(u32, u32, u32, u32)>; nexus_gfx::frame::MAX_LAYERS]);

impl CleanedRows {
    pub(super) const fn new() -> Self {
        Self([None; nexus_gfx::frame::MAX_LAYERS])
    }

    /// A new content generation: no row counts as cleaned.
    pub(super) fn forget(&mut self) {
        *self = Self::new();
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

/// Whether `q` covers any of the pass area's pixels (inclusive corners) — a draw that does not
/// writes none of them.
fn touches(q: &Quad, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> bool {
    q.w > 0.0
        && q.h > 0.0
        && q.x < (x1 + 1) as f32
        && q.x + q.w > x0 as f32
        && q.y < (y1 + 1) as f32
        && q.y + q.h > y0 as f32
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
        write_bound(self.dev, self.who.0, self.who.1, va, at, bytes).is_ok()
    }
}

/// `tex`'s descriptor written into fresh lines of the frame's buffer: its address (`none`, the
/// bundle's own, for a draw that samples nothing).
fn descriptor(
    dev: &mut Device,
    who: (ClientId, ContextId),
    rings: &mut Rings,
    tex: Tex,
    none: u64,
) -> Result<u64, &'static str> {
    let at = |va: u64, x: u32, row: u32| va + u64::from(row) * STRIDE + u64::from(x) * 4;
    let (address, width, height) = match tex {
        Tex::None => return Ok(none),
        Tex::Layout { x, row, w, h } => (at(LAYOUT_VA, x, row), w, h),
        Tex::Scratch => (SCRATCH_VA, WIDTH, PLANE_ROWS),
        Tex::Half { rows } => (HALF_VA, WIDTH, rows),
        Tex::Cache { x, row, w, h } => (at(CACHE_VA, x, row), w, h),
    };
    let image = Image {
        address,
        width,
        height,
        stride: WIDTH,
        format: format::U8U8U8U8,
        swizzle: swizzle::BGRA,
    };
    let bytes = descriptor_bytes(&image, &Sampler::BILINEAR_CLAMP).map_err(|_| "a descriptor")?;
    let off = rings.place(0, Need::Descriptor).ok_or("no line for a descriptor")?;
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
    board: &Board<'_>,
    spec: &PassSpec,
    target: Target,
    (trace, wait): (bool, bool),
    last: Option<TimelinePoint>,
) -> Result<(usize, [u64; 6], TimelinePoint), Refusal> {
    let n = board.draws(spec);
    if n == 0 || n > MAX_DRAWS {
        return Err("a pass of no draws or more than the executor holds".into());
    }
    for attempt in 0..2 {
        rings.begin();
        let ran =
            emit_and_wait(dev, who, base, t, pipes, rings, board, spec, n, target, trace, wait);
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

#[allow(clippy::too_many_arguments)]
fn emit_and_wait(
    dev: &mut Device,
    who: (ClientId, ContextId),
    base: &RenderStreams<'_>,
    t: &Template<'_>,
    pipes: &Pipes,
    rings: &mut Rings,
    board: &Board<'_>,
    spec: &PassSpec,
    n: usize,
    target: Target,
    trace: bool,
    wait: bool,
) -> Result<(TimelinePoint, [u64; 6], usize), Refusal> {
    let e0 = now_ns();
    // The draws that touch the pass's area — one outside it writes no pixel (the back end's
    // clip; TASK-0335 P4: a frame inside a window skips the shadows, the corners and the layers
    // elsewhere; the last draw stands in when none does) — their descriptors first: the emitter
    // asks for a draw more than once.
    let mut kept = [0u16; MAX_DRAWS];
    let mut tex = [0u64; MAX_DRAWS];
    let mut m = 0usize;
    for i in 0..n {
        let d = board.draw(spec, i).ok_or("a draw of the plan")?;
        if !touches(&d.quad, spec.area) && (m > 0 || i + 1 < n) {
            continue;
        }
        pipes.get(d.pipe).ok_or("a pipeline the library lacks")?;
        tex[m] = descriptor(dev, who, rings, d.tex, t.texture_va)?;
        kept[m] = i as u16;
        m += 1;
    }
    // Every draw was checked above; an empty quad keeps the closure total.
    let draw = |j: usize| {
        let d = board.draw(spec, usize::from(kept[j]));
        let pipeline = d.and_then(|d| pipes.get(d.pipe)).unwrap_or(&pipes.opaque);
        let quad = d.map_or(Quad::layer(0.0, 0.0, 0.0, 0.0, [0.0; 4], 0.0), |d| d.quad);
        Draw { pipeline, quad, texture_va: tex[j] }
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
    // The trace (the gate's last warm frame): each wait iteration's time since the submit, the
    // interrupts seen by then, and whether the geometry and the fragment words were reached.
    let mut steps = [(0u32, 0u32, 0u8); TRACE_STEPS];
    let (gtl, gseq) = dev.render_progress(who.0, who.1).map(|(g, _)| g).unwrap_or((ftl, fseq));
    let done = dev.wait_until(w0 + PASS_TIMEOUT_NS, |g, _| {
        let frag = g.mem.read_u32(&ftl, 0).wrapping_sub(fseq) < 0x8000_0000;
        if trace {
            if let Some(step) = steps.get_mut(iters as usize) {
                let geom = g.mem.read_u32(&gtl, 0).wrapping_sub(gseq) < 0x8000_0000;
                let at = (now_ns().saturating_sub(w0) / 1000) as u32;
                *step = (at, g.irqs.wrapping_sub(irq0), u8::from(geom) | (u8::from(frag) << 1));
            }
        }
        iters += 1;
        frag
    });
    let w1 = now_ns();
    dev.serve_firmware();
    let irqs = u64::from(dev.gpu.irqs.wrapping_sub(irq0));
    if trace {
        let mut line = alloc::string::String::new();
        for (i, (at, irq, words)) in
            steps.iter().enumerate().take((iters as usize).min(TRACE_STEPS))
        {
            use core::fmt::Write as _;
            let _ = write!(line, "{}{at}:{irq}:{words}", if i == 0 { "" } else { " " });
        }
        emit_fmt(format_args!(
            "gpud: gpu pass trace (iters={iters} irqs={irqs} us_at:irqs:words[g=1,f=2]= {line})"
        ));
    }
    let stamps = [s0 - e0, w0 - s0, w1 - w0, now_ns().saturating_sub(w1), iters, irqs];
    if done {
        Ok((point, stamps, m))
    } else {
        Err("a pass not reached in time".into())
    }
}
