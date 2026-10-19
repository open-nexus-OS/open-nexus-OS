// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A desktop frame's passes on the device (TASK-0215 5c-3 P4b/P5, split out of `desktop.rs`
//! under the module-size ratchet): the layout's lines cleaned for the device, the frame's pass
//! memory as the emitter's sink, a texture's descriptor in fresh lines, and one pass — its
//! descriptors, its draws emitted, submitted and waited, stamped at each step for the frame's
//! time split.

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
use super::render_cost::streams_for;
use super::render_template::{pds_at, write_bound, Target, PDS_ARENA};
use crate::gpu_context::FrameTarget;

const WIDTH: u32 = STRIDE_BYTES / 4;
const STRIDE: u64 = STRIDE_BYTES as u64;

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

/// One pass of the frame: its descriptors, its draws emitted, submitted, waited; the number of
/// draws.
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
) -> Result<(usize, [u64; 4]), Refusal> {
    let n = board.draws(spec);
    if n == 0 || n > MAX_DRAWS {
        return Err("a pass of no draws or more than the executor holds".into());
    }
    rings.begin();
    let ran = emit_and_wait(dev, who, base, t, pipes, rings, board, spec, n, target);
    match ran {
        Ok((point, mut stamps)) => {
            let r0 = now_ns();
            rings.submitted(point.0).map_err(|_| "the rings")?;
            rings.retire(&|p| dev.reached(TimelinePoint(p)));
            stamps[3] = now_ns().saturating_sub(r0);
            Ok((n, stamps))
        }
        Err(e) => {
            rings.abandon();
            Err(e)
        }
    }
}

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
) -> Result<(TimelinePoint, [u64; 4]), Refusal> {
    let e0 = now_ns();
    // The emitter asks for a draw more than once: the descriptors are written first.
    let mut tex = [0u64; MAX_DRAWS];
    for (i, t_va) in tex.iter_mut().enumerate().take(n) {
        let d = board.draw(spec, i).ok_or("a draw of the plan")?;
        pipes.get(d.pipe).ok_or("a pipeline the library lacks")?;
        *t_va = descriptor(dev, who, rings, d.tex, t.texture_va)?;
    }
    // Every draw was checked above; an empty quad keeps the closure total.
    let draw = |i: usize| {
        let d = board.draw(spec, i);
        let pipeline = d.and_then(|d| pipes.get(d.pipe)).unwrap_or(&pipes.opaque);
        let quad = d.map_or(Quad::layer(0.0, 0.0, 0.0, 0.0, [0.0; 4], 0.0), |d| d.quad);
        Draw { pipeline, quad, texture_va: tex[i] }
    };
    let mut sink = FrameSink { dev: &mut *dev, who };
    let emitted = emit_draws(t, n, &draw, rings, &mut sink).map_err(|e| match e {
        EmitError::Space => "the pass's emit: no room in the pass memory",
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
    let w0 = now_ns();
    let done = dev.wait_until(w0 + PASS_TIMEOUT_NS, |g, _| {
        g.mem.read_u32(&ftl, 0).wrapping_sub(fseq) < 0x8000_0000
    });
    let w1 = now_ns();
    dev.serve_firmware();
    let stamps = [s0 - e0, w0 - s0, w1 - w0, now_ns().saturating_sub(w1)];
    if done {
        Ok((point, stamps))
    } else {
        Err("a pass not reached in time".into())
    }
}
