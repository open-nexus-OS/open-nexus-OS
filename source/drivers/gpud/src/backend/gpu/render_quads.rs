// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Dynamic geometry on the board (TASK-0215 5b, RFC-0101 §8d): textured, tinted quads at
//! positions the device chooses, drawn through the bindings and render objects the render rung
//! left with the library's premultiplied layer (5c-2 P4d: the setup programs ours, placed once
//! by the library rung). Every pass lands in fresh memory the GPU has not read
//! (`nexus_gfx::backend::gpu::quads`): the control stream, the vertices, the vertex PDS data,
//! each draw's blocks; the geometry job's `vdm_ctrl_stream_base` is patched to the pass's stream.
//! The target is rebound at a fresh plane once; a sample inside each quad is read back against
//! the formula (texel × tint as a premultiplied source over the clear). Probes, each a line,
//! keep the diagnosis sharp: one quad (data 1), one again (data 0), two, three (the gate):
//! `gpud: gpu quads ok (n=3 us= samples=3/3)`.

use nexus_driverkit::gpu::GpuVa;
use nexus_gfx::backend::gpu::fwif::GeomCmd;
use nexus_gfx::backend::gpu::pipelines::Placed;
use nexus_gfx::backend::gpu::quads::{self, Bump, Placer, Quad, Template};
use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{
    Batch, ClientId, ContextId, GfxDevice, QueueKind, RenderStreams, RenderTarget, Stream,
    TimelinePoint,
};

use super::device_v2::Device;
use super::emit;
use super::render_template::{
    premultiplied_bgr, render_streams, Captured, PassSink, RungContext, GENERAL_ARENA, QUADS_TAILS,
    SAMPLE_FRACTION, STREAM_BUFFER, VERTEX_BUFFER,
};
use crate::backend::display::{Display, GpuCanvas};

const BUNDLE: &[u8] = super::render_template::RENDER_BUNDLE;
const PASS_TIMEOUT_NS: u64 = 2_000_000_000;
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const STRIDE: usize = 7680;
const TOLERANCE: u8 = 4;
/// The library pipeline the quads draw with (`resources/gpu/pipelines/compositor.toml`).
const LAYER: &str = "layer-premultiplied";
/// The three quads: the capture's positions and tints.
const QUADS: [Quad; 3] = [
    Quad::layer(704.0, 284.0, 512.0, 512.0, [1.0, 1.0, 1.0, 1.0], 0.0),
    Quad::layer(304.0, 134.0, 512.0, 512.0, [1.0, 0.5, 0.5, 1.0], 0.0),
    Quad::layer(1104.0, 434.0, 512.0, 512.0, [0.5, 0.5, 1.0, 0.6], 0.0),
];

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// One pass of `quads` placed by `place`: the writes, the kick, the samples. `Ok((us, irqs))`.
#[allow(clippy::too_many_arguments)]
fn probe(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    client: ClientId,
    ctx: ContextId,
    t: &Template<'_>,
    streams: &RenderStreams<'_>,
    target_va: u64,
    pipeline: &Placed,
    quads: &[Quad],
    place: &mut dyn Placer,
) -> Result<(u64, u32), alloc::string::String> {
    let mut sink = PassSink { dev: &mut *dev, client, ctx, refused: None };
    let emitted = match quads::emit(t, pipeline, quads, place, &mut sink) {
        Ok(e) => e,
        Err(e) => {
            return Err(match sink.refused.take() {
                Some(w) => alloc::format!("write {w}"),
                None => alloc::format!("emit {e:?}"),
            })
        }
    };
    if canvas.block != 0 {
        if let Some(bytes) = display.gpu_canvas_bytes(canvas.row) {
            nexus_abi::cache_clean(bytes, canvas.block);
        }
    }
    // The geometry job names the pass's stream.
    let feats = dev.gpu.fw.render;
    let mut g =
        GeomCmd::from_stream(streams.geom, feats, streams.geom_flags, 0, Default::default())
            .map_err(|e| alloc::format!("geometry stream {e:?}"))?;
    g.regs.vdm_ctrl_stream_base = emitted.vdm_ctrl_stream_base;
    let geom = g.to_stream(feats);
    let pass = RenderStreams { geom: &geom, ..*streams };
    let batch = Batch {
        target: RenderTarget { va: GpuVa(target_va), width: WIDTH, height: HEIGHT },
        stream: Stream::Render(pass),
        waits: &[],
        signal: TimelinePoint(0),
    };
    let irqs_before = dev.gpu.irqs;
    let t0 = now_ns();
    let point = dev
        .submit(client, ctx, QueueKind::Render, &batch)
        .map_err(|e| alloc::format!("submit {e:?}"))?;
    if !dev.wait_point(point, t0 + PASS_TIMEOUT_NS) {
        dev.serve_firmware();
        let (gw, fw) = dev.render_words(client, ctx).unwrap_or((0, 0));
        return Err(alloc::format!(
            "pass not done in {}us (point {:#x} geom_word={gw} frag_word={fw} irqs={})",
            now_ns().saturating_sub(t0) / 1000,
            point.0,
            dev.gpu.irqs - irqs_before
        ));
    }
    let us = now_ns().saturating_sub(t0) / 1000;
    let irqs = dev.gpu.irqs - irqs_before;
    dev.serve_firmware();
    let bytes = display.gpu_canvas_bytes(canvas.row).ok_or("no cpu view of the plane")?;
    if canvas.block != 0 {
        nexus_abi::cache_flush(bytes, canvas.block);
    }
    let mut good = 0;
    let mut bad = None;
    for (i, q) in quads.iter().enumerate() {
        let x = (q.x + SAMPLE_FRACTION * q.w) as usize;
        let y = (q.y + SAMPLE_FRACTION * q.h) as usize;
        let want = premultiplied_bgr(q.tint());
        let at = y * STRIDE + x * 4;
        let Some(px) = bytes.get(at..at + 3) else {
            bad.get_or_insert((i, x, y, want, [0u8; 3]));
            continue;
        };
        if px.iter().zip(&want).all(|(p, w)| p.abs_diff(*w) <= TOLERANCE) {
            good += 1;
        } else {
            bad.get_or_insert((i, x, y, want, [px[0], px[1], px[2]]));
        }
    }
    match bad {
        None => Ok((us, irqs)),
        Some((i, x, y, want, got)) => Err(alloc::format!(
            "quad {i} sample ({x},{y}) expected bgr {:02x}{:02x}{:02x} read {:02x}{:02x}{:02x}; {good}/{} us={us} irqs={irqs}",
            want[0],
            want[1],
            want[2],
            got[0],
            got[1],
            got[2],
            quads.len()
        )),
    }
}

/// The rung: after `gpu render ok`, in the render rung's context with its objects.
#[inline(never)]
pub(crate) fn rung(dev: &mut Device, display: &mut dyn Display, rc: &mut RungContext) {
    let fail = |what: &str| emit(&alloc::format!("gpud: FAIL gpu quads ({what})"));
    let core = CoreId(dev.gpu.win.core_id());
    let scene = match Scene::parse(BUNDLE, core) {
        Ok(s) => s,
        Err(e) => return fail(&alloc::format!("scene refused {e:?}")),
    };
    let (Some(target), Some(streams)) = (scene.target(), render_streams(&scene)) else {
        return fail("scene without a target or the three render jobs");
    };
    let (client, ctx) = (rc.client, rc.ctx);
    let captured = match Captured::read(dev, client, ctx) {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    let template = captured.template(GENERAL_ARENA, VERTEX_BUFFER, STREAM_BUFFER);
    let Some(pipeline) = dev.pipeline(client, ctx, LAYER) else {
        return fail("the library's premultiplied layer is not in the context");
    };
    // A fresh plane under the target address.
    let Some(canvas) = display.gpu_canvas() else {
        return fail("no plane of the ring free");
    };
    if let Err(e) = rc.scene.retarget(dev, client, ctx, canvas.bus) {
        display.gpu_canvas_discard(canvas.row);
        return fail(&alloc::format!("rebind target {e:?}"));
    }
    // Fresh lines for every pass: this rung's halves of the bundle's buffers' tails.
    let mut place: Bump = QUADS_TAILS;
    let probes: [(&str, &[Quad]); 4] = [
        ("one", &QUADS[..1]),
        ("one-again", &QUADS[..1]),
        ("two", &QUADS[..2]),
        ("three", &QUADS[..3]),
    ];
    let mut gate = Err(alloc::string::String::from("no probe ran"));
    for (name, quads) in probes {
        let r = probe(
            dev, display, &canvas, client, ctx, &template, &streams, target.va, &pipeline, quads,
            &mut place,
        );
        match &r {
            Ok((us, irqs)) => emit(&alloc::format!(
                "gpud: gpu quads probe {name} ok (n={} us={us} irqs={irqs})",
                quads.len()
            )),
            Err(e) => emit(&alloc::format!("gpud: gpu quads probe {name} FAIL ({e})")),
        }
        if name == "three" {
            gate = r;
        }
    }
    match gate {
        Ok((us, irqs)) => {
            if let Err(e) = display.gpu_canvas_show(canvas.row) {
                return fail(&alloc::format!("the plane could not be shown {e:?}"));
            }
            emit(&alloc::format!(
                "gpud: gpu quads ok (n=3 us={us} samples=3/3 irqs={irqs} shown row={})",
                canvas.row
            ));
        }
        Err(e) => {
            display.gpu_canvas_discard(canvas.row);
            fail(&e);
        }
    }
}
