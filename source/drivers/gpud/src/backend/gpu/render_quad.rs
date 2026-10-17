// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The render queue's first pass (TASK-0215 5a, RFC-0101 §8d): the open driver's captured
//! render pass — a clear and one blended textured quad, three jobs — replayed through
//! `GfxDevice` into a plane of the ring, with the render context, the free lists and the
//! dataset built by the device. Runs after the transfer quad, in a context of its own (the two
//! captures share addresses with different bytes): the render bundle loaded (`scene_load`), the
//! pass kicked, four sample pixels read back — the background, the quad's white frame at alpha
//! 0.99, two checker colours at their alpha — and the plane shown: `gpud: gpu render ok (us=
//! samples=4/4 irqs= geom= frag=)`. The context stays for the rungs after it ([`RungContext`]).

use nexus_driverkit::gpu::GpuVa;
use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{
    Batch, ClientId, ContextId, GfxDevice, Priority, QueueKind, RenderTarget, Stream, TimelinePoint,
};

use super::device_v2::Device;
use super::emit;
use super::render_template::{render_streams, RungContext};
use super::scene_load::{self, TargetMem, Texel};
use crate::backend::display::{Display, GpuCanvas};

const BUNDLE: &[u8] = super::render_template::RENDER_BUNDLE;
const PASS_TIMEOUT_NS: u64 = 2_000_000_000;
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
pub(super) const STRIDE: usize = 7680;
/// Sample pixels (x, y, expected memory bytes B, G, R): the clear (0.10, 0.12, 0.16); the
/// texture's white frame blended at alpha 252/255; checker (40,90,200) at alpha 215/255;
/// checker (230,120,30) at alpha 188/255 — `docs/board/measurements/2026-10-13-gpu-render-queue/`.
pub(super) const SAMPLES: [(u32, u32, [u8; 3]); 4] = [
    (10, 10, [0x29, 0x1f, 0x1a]),
    (1210, 790, [0xfc, 0xfc, 0xfc]),
    (1150, 700, [0xaf, 0x51, 0x26]),
    (1040, 700, [0x21, 0x61, 0xb0]),
];
pub(super) const TOLERANCE: u8 = 4;

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

fn fail(what: &str) {
    emit(&alloc::format!("gpud: FAIL gpu render ({what})"));
}

/// The rung: after `gpu quad ok`, with the display's ring; the context it made, for the rungs
/// after it.
#[inline(never)]
pub(crate) fn rung(dev: &mut Device, display: &mut dyn Display) -> Option<RungContext> {
    let core = CoreId(dev.gpu.win.core_id());
    let scene = match Scene::parse(BUNDLE, core) {
        Ok(s) => s,
        Err(e) => {
            fail(&alloc::format!("scene refused {e:?}"));
            return None;
        }
    };
    let client = ClientId(nexus_abi::service_id_from_name(b"gpud"));
    let ctx = match dev.create_context(client, Priority::Realtime) {
        Ok(c) => c,
        Err(e) => {
            fail(&alloc::format!("context {e:?}"));
            return None;
        }
    };
    let Some(canvas) = display.gpu_canvas() else {
        let _ = dev.destroy_context(client, ctx);
        fail("no plane of the ring free");
        return None;
    };
    let t_bind = now_ns();
    let target = TargetMem::Plane { bus: canvas.bus };
    let loaded = match scene_load::load(dev, client, ctx, &scene, target, Texel::Checker) {
        Ok(l) => l,
        Err(e) => {
            let _ = dev.destroy_context(client, ctx);
            display.gpu_canvas_discard(canvas.row);
            fail(&e);
            return None;
        }
    };
    let r = match scene.render() {
        Some(setup) => dev
            .render_setup(client, ctx, &setup)
            .map_err(|e| alloc::format!("render setup {e:?}"))
            .and_then(|()| {
                let bind_us = now_ns().saturating_sub(t_bind) / 1000;
                pass(dev, display, client, ctx, &scene, canvas, bind_us)
            }),
        None => Err(alloc::string::String::from("scene without the render trailer")),
    };
    match r {
        Ok(line) => {
            emit(&line);
            Some(RungContext { client, ctx, scene: loaded })
        }
        Err(e) => {
            let _ = dev.destroy_context(client, ctx);
            loaded.release(dev, client, ctx);
            display.gpu_canvas_discard(canvas.row);
            fail(&e);
            None
        }
    }
}

/// The captured pass into the plane, the samples read, the plane shown; the gate's line.
fn pass(
    dev: &mut Device,
    display: &mut dyn Display,
    client: ClientId,
    ctx: ContextId,
    scene: &Scene<'_>,
    canvas: GpuCanvas,
    bind_us: u64,
) -> Result<alloc::string::String, alloc::string::String> {
    let target = scene.target().ok_or("scene without a target")?;
    let streams = render_streams(scene).ok_or("the bundle lacks one of the three render jobs")?;
    if canvas.block != 0 {
        if let Some(bytes) = display.gpu_canvas_bytes(canvas.row) {
            nexus_abi::cache_clean(bytes, canvas.block);
        }
    }
    let batch = Batch {
        target: RenderTarget { va: GpuVa(target.va), width: WIDTH, height: HEIGHT },
        stream: Stream::Render(streams),
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
    let (gw, fw) = dev.render_words(client, ctx).unwrap_or((0, 0));
    let bytes = display.gpu_canvas_bytes(canvas.row).ok_or("no cpu view of the plane")?;
    if canvas.block != 0 {
        nexus_abi::cache_flush(bytes, canvas.block);
    }
    let (good, bad) = check(bytes, &SAMPLES);
    if let Some((i, x, y, want, got)) = bad {
        return Err(alloc::format!(
            "sample {i} at ({x},{y}) expected bgr {:02x}{:02x}{:02x} read {:02x}{:02x}{:02x}; {good}/4 us={us} irqs={irqs} geom_word={gw} frag_word={fw}",
            want[0], want[1], want[2], got[0], got[1], got[2]
        ));
    }
    display
        .gpu_canvas_show(canvas.row)
        .map_err(|e| alloc::format!("the plane could not be shown {e:?}"))?;
    Ok(alloc::format!(
        "gpud: gpu render ok (us={us} samples={good}/4 irqs={irqs} geom={gw} frag={fw} bind_us={bind_us} shown row={})",
        canvas.row
    ))
}

/// The samples of a picture of the plane's stride: how many read their bytes, the first that
/// did not `(index, x, y, wanted, read)`.
#[allow(clippy::type_complexity)]
pub(super) fn check(
    bytes: &[u8],
    samples: &[(u32, u32, [u8; 3])],
) -> (usize, Option<(usize, u32, u32, [u8; 3], [u8; 3])>) {
    let mut good = 0;
    let mut bad = None;
    for (i, (x, y, want)) in samples.iter().enumerate() {
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
    (good, bad)
}
