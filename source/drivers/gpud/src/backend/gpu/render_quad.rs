// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The render queue's first pass (TASK-0215 5a, RFC-0101 §8d): the open driver's captured
//! render pass — a clear and one blended textured quad, three jobs — replayed through
//! `GfxDevice` into a plane of the ring, with the render context, the free lists and the
//! dataset built by the device. Runs after the transfer quad: the layer-4 bindings are dropped
//! with their context (the two captures share addresses with different bytes), the render
//! bundle is bound, the pass kicked, four sample pixels read back — the background, the quad's
//! white frame at alpha 0.99, two checker colours at their alpha — and the plane shown:
//! `gpud: gpu render ok (us= samples=4/4 irqs= geom= frag=)`.

use nexus_driverkit::gpu::GpuVa;
use nexus_gfx::backend::gpu::scene::{JobKind, Scene};
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{
    Batch, ClientId, ContextId, GfxDevice, Priority, QueueKind, RenderStreams, RenderTarget,
    Stream, TimelinePoint,
};

use super::device_v2::Device;
use super::emit;
use super::quad::bind;
use crate::backend::display::Display;

const BUNDLE: &[u8] = include_bytes!("../../../../../../resources/gpu/scenes/render-quad.scene");
const PASS_TIMEOUT_NS: u64 = 2_000_000_000;
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const STRIDE: usize = 7680;
/// Sample pixels (x, y, expected memory bytes B, G, R): the clear (0.10, 0.12, 0.16); the
/// texture's white frame blended at alpha 252/255; checker (40,90,200) at alpha 215/255;
/// checker (230,120,30) at alpha 188/255 — `docs/board/measurements/2026-10-13-gpu-render-queue/`.
const SAMPLES: [(u32, u32, [u8; 3]); 4] = [
    (10, 10, [0x29, 0x1f, 0x1a]),
    (1210, 790, [0xfc, 0xfc, 0xfc]),
    (1150, 700, [0xaf, 0x51, 0x26]),
    (1040, 700, [0x21, 0x61, 0xb0]),
];
const TOLERANCE: u8 = 4;

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

fn fail(what: &str) {
    emit(&alloc::format!("gpud: FAIL gpu render ({what})"));
}

/// The rung: after `gpu quad ok`, with the display's ring.
#[inline(never)]
pub(crate) fn rung(dev: &mut Device, display: &mut dyn Display) {
    let core = CoreId(dev.gpu.win.core_id());
    let scene = match Scene::parse(BUNDLE, core) {
        Ok(s) => s,
        Err(e) => return fail(&alloc::format!("scene refused {e:?}")),
    };
    let (Some(target), Some(setup)) = (scene.target(), scene.render()) else {
        return fail("scene without a target or the render trailer");
    };
    let client = ClientId(nexus_abi::service_id_from_name(b"gpud"));
    // Layer 4's context goes with its bindings: the two captures share addresses.
    let _ = dev.destroy_context(client, ContextId(1));
    let ctx = match dev.create_context(client, Priority::Realtime) {
        Ok(c) => c,
        Err(e) => return fail(&alloc::format!("context {e:?}")),
    };
    let Some(canvas) = display.gpu_canvas() else {
        return fail("no plane of the ring free");
    };
    let t_bind = now_ns();
    for buf in scene.buffers() {
        if let Err(e) = bind(dev, client, ctx, &buf, canvas.bus) {
            display.gpu_canvas_discard(canvas.row);
            return fail(&e);
        }
    }
    if let Err(e) = dev.render_setup(client, ctx, &setup) {
        display.gpu_canvas_discard(canvas.row);
        return fail(&alloc::format!("render setup {e:?}"));
    }
    let bind_us = now_ns().saturating_sub(t_bind) / 1000;
    if canvas.block != 0 {
        if let Some(bytes) = display.gpu_canvas_bytes(canvas.row) {
            nexus_abi::cache_clean(bytes, canvas.block);
        }
    }
    let mut geom = None;
    let mut frag_pr = None;
    let mut frag = None;
    for job in scene.jobs() {
        match job.kind {
            JobKind::Geometry => geom = Some(job),
            JobKind::FragmentPartial => frag_pr = Some(job),
            JobKind::Fragment => frag = Some(job),
            JobKind::Transfer => {}
        }
    }
    let (Some(g), Some(pr), Some(f)) = (geom, frag_pr, frag) else {
        display.gpu_canvas_discard(canvas.row);
        return fail("the bundle lacks one of the three render jobs");
    };
    let streams = RenderStreams {
        geom: g.stream,
        geom_flags: g.flags,
        frag_pr: pr.stream,
        frag_pr_flags: pr.flags,
        frag: f.stream,
        frag_flags: f.flags,
    };
    let batch = Batch {
        target: RenderTarget { va: GpuVa(target.va), width: WIDTH, height: HEIGHT },
        stream: Stream::Render(streams),
        waits: &[],
        signal: TimelinePoint(0),
    };
    let irqs_before = dev.gpu.irqs;
    let t0 = now_ns();
    let point = match dev.submit(client, ctx, QueueKind::Render, &batch) {
        Ok(p) => p,
        Err(e) => {
            display.gpu_canvas_discard(canvas.row);
            return fail(&alloc::format!("submit {e:?}"));
        }
    };
    if !dev.wait_point(point, t0 + PASS_TIMEOUT_NS) {
        dev.gpu.drain_fwccb();
        let (gw, fw) = dev.render_words().unwrap_or((0, 0));
        display.gpu_canvas_discard(canvas.row);
        return fail(&alloc::format!(
            "pass not done in {}us (point {:#x} geom_word={gw} frag_word={fw} irqs={})",
            now_ns().saturating_sub(t0) / 1000,
            point.0,
            dev.gpu.irqs - irqs_before
        ));
    }
    let us = now_ns().saturating_sub(t0) / 1000;
    let irqs = dev.gpu.irqs - irqs_before;
    let (gw, fw) = dev.render_words().unwrap_or((0, 0));
    let Some(bytes) = display.gpu_canvas_bytes(canvas.row) else {
        return fail("no cpu view of the plane");
    };
    if canvas.block != 0 {
        nexus_abi::cache_flush(bytes, canvas.block);
    }
    let mut good = 0;
    let mut bad = None;
    for (i, (x, y, want)) in SAMPLES.iter().enumerate() {
        let at = *y as usize * STRIDE + *x as usize * 4;
        let Some(px) = bytes.get(at..at + 3) else {
            bad.get_or_insert((i, *x, *y, *want, [0u8; 3]));
            continue;
        };
        if px.iter().zip(want).all(|(p, w)| p.abs_diff(*w) <= TOLERANCE) {
            good += 1;
        } else {
            bad.get_or_insert((i, *x, *y, *want, [px[0], px[1], px[2]]));
        }
    }
    if let Some((i, x, y, want, got)) = bad {
        display.gpu_canvas_discard(canvas.row);
        return fail(&alloc::format!(
            "sample {i} at ({x},{y}) expected bgr {:02x}{:02x}{:02x} read {:02x}{:02x}{:02x}; {good}/4 us={us} irqs={irqs} geom_word={gw} frag_word={fw}",
            want[0], want[1], want[2], got[0], got[1], got[2]
        ));
    }
    if let Err(e) = display.gpu_canvas_show(canvas.row) {
        return fail(&alloc::format!("the plane could not be shown {e:?}"));
    }
    emit(&alloc::format!(
        "gpud: gpu render ok (us={us} samples={good}/4 irqs={irqs} geom={gw} frag={fw} bind_us={bind_us} shown row={})",
        canvas.row
    ));
}
