// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The first quad (TASK-0169B P2, RFC-0101 §8c): the open driver's two transfer jobs for this
//! core, captured on the host as a scene bundle, replayed through `GfxDevice` into a plane of
//! the ring. Every buffer of the bundle is created, filled (the texture by its formula) and
//! bound at its captured address; the plane is adopted at the target's address; the clear is
//! timed against the stock stack's 1.8 ns a pixel (`gpud: gpu fill ok (ns_per_px=)`); after
//! the blit four sample pixels are read back — the background, the texture's white frame, the
//! two checker colours — and the plane goes to the glass (`gpud: gpu quad ok (samples=4/4)`).
//! A FAIL form names the sample (expected, read) or the step that refused.

use nexus_driverkit::gpu::{Access, CacheMode, GpuVa};
use nexus_gfx::backend::gpu::scene::{self, Kind, Scene};
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{
    Batch, BoId, ClientId, GfxDevice, Priority, QueueKind, RenderTarget, Stream, TimelinePoint,
};

use super::device_v2::Device;
use super::emit;
use crate::backend::display::Display;

const BUNDLE: &[u8] = include_bytes!("../../../../../../resources/gpu/scenes/first-quad.scene");
/// One job's budget.
const JOB_TIMEOUT_NS: u64 = 2_000_000_000;
/// The bar: the stock stack's fill cost, in tenths of a nanosecond a pixel.
const STOCK_NS_PER_PX_X10: u64 = 18;
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const STRIDE: usize = 7680;
/// Sample pixels of the picture (x, y, expected memory bytes B, G, R): the clear colour
/// (0.10, 0.12, 0.16) at a corner; the texture's white frame at the quad's left edge; the two
/// checker colours inside two blocks (`docs/board/measurements/2026-10-13-gpu-first-quad/`).
const SAMPLES: [(u32, u32, [u8; 3]); 4] = [
    (10, 10, [0x29, 0x1f, 0x1a]),
    (705, 540, [255, 255, 255]),
    (744, 324, [200, 90, 40]),
    (809, 325, [30, 120, 230]),
];
const TOLERANCE: u8 = 2;

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

fn fail(what: &str) {
    emit(&alloc::format!("gpud: FAIL gpu quad ({what})"));
}

/// Creates, fills and binds one buffer of the bundle; the target is the adopted plane.
fn bind(
    dev: &mut Device,
    client: ClientId,
    ctx: nexus_gfx::backend::v2::ContextId,
    buf: &scene::Buffer<'_>,
    plane_bus: u64,
) -> Result<BoId, alloc::string::String> {
    let len = buf.len as usize;
    let (bo, access) = match buf.kind {
        Kind::Target => (
            dev.adopt_plane(client, plane_bus, len).map_err(|e| alloc::format!("adopt {e:?}"))?,
            Access::ReadWrite,
        ),
        Kind::Data | Kind::Texture => {
            let bo = dev
                .create_bo(client, len, CacheMode::CpuCached)
                .map_err(|e| alloc::format!("create_bo {len} {e:?}"))?;
            let bytes = dev.bo_bytes_mut(client, bo).ok_or("no cpu view")?;
            match buf.kind {
                Kind::Texture => scene::checker_v1(256, 256, 1024, bytes),
                _ => {
                    for run in buf.runs() {
                        let at = run.off as usize;
                        if let Some(dst) = bytes.get_mut(at..at + run.bytes.len()) {
                            dst.copy_from_slice(run.bytes);
                        }
                    }
                }
            }
            dev.clean(client, bo);
            (bo, Access::ReadOnly)
        }
    };
    dev.vm_bind(client, ctx, bo, GpuVa(buf.va), access)
        .map_err(|e| alloc::format!("bind {:#x} {e:?}", buf.va))?;
    Ok(bo)
}

/// The rung: after `gpu fence ok`, with the display's ring.
#[inline(never)]
pub(crate) fn rung(dev: &mut Device, display: &mut dyn Display) {
    let core = CoreId(dev.gpu.win.core_id());
    let scene = match Scene::parse(BUNDLE, core) {
        Ok(s) => s,
        Err(e) => return fail(&alloc::format!("scene refused {e:?}")),
    };
    let Some(target) = scene.target() else {
        return fail("scene without a target");
    };
    let client = ClientId(nexus_abi::service_id_from_name(b"gpud"));
    let ctx = match dev.create_context(client, Priority::Realtime) {
        Ok(c) => c,
        Err(e) => return fail(&alloc::format!("context {e:?}")),
    };
    let Some(canvas) = display.gpu_canvas() else {
        return fail("no plane of the ring free");
    };
    if (canvas.len as u64) < target.len {
        display.gpu_canvas_discard(canvas.row);
        return fail(&alloc::format!("plane {} < target {}", canvas.len, target.len));
    }
    let t_bind = now_ns();
    for buf in scene.buffers() {
        if let Err(e) = bind(dev, client, ctx, &buf, canvas.bus) {
            display.gpu_canvas_discard(canvas.row);
            return fail(&e);
        }
    }
    let bind_us = now_ns().saturating_sub(t_bind) / 1000;
    // Ownership of the plane CPU → device: the lines the CPU wrote (the splash copy, the
    // composite) are cleaned before the GPU writes under them (H4).
    if canvas.block != 0 {
        if let Some(bytes) = display.gpu_canvas_bytes(canvas.row) {
            nexus_abi::cache_clean(bytes, canvas.block);
        }
    }
    let render_target = RenderTarget { va: GpuVa(target.va), width: WIDTH, height: HEIGHT };
    let mut us = [0u64; 2];
    for (i, job) in scene.jobs().enumerate().take(2) {
        let t0 = now_ns();
        let batch = Batch {
            target: render_target,
            stream: Stream::Transfer { stream: job.stream, single_core: job.single_core() },
            waits: &[],
            signal: TimelinePoint(0),
        };
        let point = match dev.submit(client, ctx, QueueKind::Transfer, &batch) {
            Ok(p) => p,
            Err(e) => {
                display.gpu_canvas_discard(canvas.row);
                return fail(&alloc::format!("submit {i} {e:?}"));
            }
        };
        if !dev.wait_point(point, t0 + JOB_TIMEOUT_NS) {
            dev.gpu.drain_fwccb();
            display.gpu_canvas_discard(canvas.row);
            return fail(&alloc::format!(
                "job {i} not done in {}us (point {} irqs={})",
                now_ns().saturating_sub(t0) / 1000,
                point.0,
                dev.gpu.irqs
            ));
        }
        us[i] = now_ns().saturating_sub(t0) / 1000;
        if i == 0 {
            let px = u64::from(WIDTH) * u64::from(HEIGHT);
            let x10 = us[0] * 10_000 / px;
            let verdict = if x10 <= STOCK_NS_PER_PX_X10 { "at or below" } else { "ABOVE" };
            emit(&alloc::format!(
                "gpud: gpu fill ok (us={} px={WIDTH}x{HEIGHT} ns_per_px={}.{} {verdict} the stock 1.8 bind_us={bind_us})",
                us[0],
                x10 / 10,
                x10 % 10
            ));
        }
    }
    // Ownership device → CPU for the readback: flush drops the CPU's stale lines.
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
        let close = px.iter().zip(want).all(|(p, w)| p.abs_diff(*w) <= TOLERANCE);
        if close {
            good += 1;
        } else {
            bad.get_or_insert((i, *x, *y, *want, [px[0], px[1], px[2]]));
        }
    }
    if let Some((i, x, y, want, got)) = bad {
        display.gpu_canvas_discard(canvas.row);
        return fail(&alloc::format!(
            "sample {i} at ({x},{y}) expected bgr {:02x}{:02x}{:02x} read {:02x}{:02x}{:02x}; {good}/4 blit_us={}",
            want[0], want[1], want[2], got[0], got[1], got[2], us[1]
        ));
    }
    if let Err(e) = display.gpu_canvas_show(canvas.row) {
        return fail(&alloc::format!("the plane could not be shown {e:?}"));
    }
    emit(&alloc::format!(
        "gpud: gpu quad ok (us={} samples={good}/4 shown row={})",
        us[1],
        canvas.row
    ));
}
