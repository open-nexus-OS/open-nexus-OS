// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Several contexts on the board (TASK-0215 5c-3 P2b, RFC-0101 §1/§6). The rungs' context goes
//! first — its render objects and transfer queue let go by the firmware's cleanup, its space and
//! its scene given back, the device holding what the ladder left it (`backing=` the free pages
//! once the G1 ladder ended). Then two contexts of two clients, side by side, twice: each its own
//! space; the render bundle loaded into each at the same addresses — A's target a plane of the
//! ring, B's a buffer of B's own, B's texture the checker with red and blue exchanged; both
//! passes in flight together. Each picture is its context's own — A's samples in the plane, B's
//! in its buffer (the same address reached other memory, for reads and writes) — and each
//! context is refused the other's buffers and addresses by name. Both go, every page back. The
//! second round proves the slots serve again: the first round's numbers refused, gpud's heap
//! the same before and after it (`heap_delta=0`). `gpud: gpu contexts ok (`.

use nexus_display_proto::layout::PLANE_BYTES;
use nexus_driverkit::gpu::{Access, CacheMode, GpuVa};
use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{
    Batch, BoId, ClientId, ContextId, DeviceError, GfxDevice, Mapping, Priority, QueueKind,
    RenderTarget, Stream, TimelinePoint,
};

use super::device_v2::Device;
use super::emit;
use super::render_quad::{check, SAMPLES};
use super::render_template::{render_streams, RungContext};
use super::scene_load::{self, Loaded, TargetMem, Texel};
use crate::backend::display::{Display, GpuCanvas};

const BUNDLE: &[u8] = include_bytes!("../../../../../../resources/gpu/scenes/render-quad.scene");
const PASS_TIMEOUT_NS: u64 = 2_000_000_000;
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
/// B's samples: the clear and the white frame as A's; the two checker colours with red and
/// blue exchanged — (200, 90, 40) at alpha 215/255 and (30, 120, 230) at 188/255 over the
/// clear, straight alpha as the captured pass blends (B, G, R in memory).
const SAMPLES_SWAPPED: [(u32, u32, [u8; 3]); 4] =
    [SAMPLES[0], SAMPLES[1], (1150, 700, [0x28, 0x51, 0xad]), (1040, 700, [0xb4, 0x61, 0x1d])];
/// An address B binds and A does not (above the bundle's heaps).
const B_ONLY_VA: u64 = 0x90_0000_0000;

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// What a round measured.
#[derive(Default)]
struct Round {
    both_us: u64,
    cleanup_us: u64,
    samples: usize,
    refused: usize,
    catalogues: (u64, u64),
}

/// A context of the round: its client, its number, its scene.
struct Side {
    client: ClientId,
    ctx: ContextId,
    scene: Option<Loaded>,
}

/// The rung: after the render rungs, with their context.
#[inline(never)]
pub(crate) fn rung(dev: &mut Device, display: &mut dyn Display, rc: Option<RungContext>) {
    let fail = |what: &str| emit(&alloc::format!("gpud: FAIL gpu contexts ({what})"));
    let t0 = now_ns();
    if let Some(rc) = rc {
        if let Err(e) = dev.destroy_context(rc.client, rc.ctx) {
            return fail(&alloc::format!("the rungs' context {e:?}"));
        }
        rc.scene.release(dev, rc.client, rc.ctx);
    }
    let teardown_us = now_ns().saturating_sub(t0) / 1000;
    let baseline = dev.gpu.baseline_free;
    let h = dev.holdings();
    if h.live != 0 || h.bos != 0 || h.backing_free != baseline {
        return fail(&alloc::format!(
            "the ladder's contexts were not given back (live={} bos={} backing={}/{baseline})",
            h.live,
            h.bos,
            h.backing_free
        ));
    }
    let core = CoreId(dev.gpu.win.core_id());
    let Ok(scene) = Scene::parse(BUNDLE, core) else {
        return fail("scene refused");
    };
    let mut stale = None;
    let mut first = Round::default();
    let mut heap_before = 0;
    for round in 0..2 {
        if round == 1 {
            heap_before = nexus_service_entry::os::heap_cursor().1;
        }
        let Some(canvas) = display.gpu_canvas() else {
            return fail("no plane of the ring free");
        };
        let r = match side_by_side(dev, display, &scene, canvas, stale) {
            Ok((r, ids)) => {
                stale = Some(ids);
                r
            }
            Err(e) => {
                display.gpu_canvas_discard(canvas.row);
                return fail(&alloc::format!("round {round}: {e}"));
            }
        };
        let h = dev.holdings();
        if h.live != 0 || h.bos != 0 || h.backing_free != baseline || h.quarantined != 0 {
            display.gpu_canvas_discard(canvas.row);
            return fail(&alloc::format!(
                "round {round} left (live={} bos={} backing={}/{baseline} quarantined={})",
                h.live,
                h.bos,
                h.backing_free,
                h.quarantined
            ));
        }
        if round == 0 {
            display.gpu_canvas_discard(canvas.row);
            first = r;
            continue;
        }
        // Measured before any line is formatted: a line takes gpud's heap.
        let heap_delta = nexus_service_entry::os::heap_cursor().1.saturating_sub(heap_before);
        let shown = display.gpu_canvas_show(canvas.row).is_ok();
        let line = alloc::format!(
            "(rounds=2 contexts=2 samples={}/16 refused={}/20 both_us={}/{} cleanup_us={}/{} teardown_us={teardown_us} pc_a={:#x} pc_b={:#x} backing={}/{baseline} bos=0 quarantined=0 cleanup_busy={} heap_delta={heap_delta} shown={shown})",
            first.samples + r.samples,
            first.refused + r.refused,
            first.both_us,
            r.both_us,
            first.cleanup_us,
            r.cleanup_us,
            r.catalogues.0,
            r.catalogues.1,
            h.backing_free,
            dev.gpu.cleanup_busy
        );
        if first.samples + r.samples == 16 && first.refused + r.refused == 20 && heap_delta == 0 {
            emit(&alloc::format!("gpud: gpu contexts ok {line}"));
        } else {
            fail(&line);
        }
    }
}

/// One round: A and B made, loaded, set up, both passes in flight, the pictures read, the
/// refusals asked, both torn down; the round's numbers and the two context numbers.
fn side_by_side(
    dev: &mut Device,
    display: &mut dyn Display,
    scene: &Scene<'_>,
    canvas: GpuCanvas,
    stale: Option<[ContextId; 2]>,
) -> Result<(Round, [ContextId; 2]), alloc::string::String> {
    let mut a = Side {
        client: ClientId(nexus_abi::service_id_from_name(b"gpud")),
        ctx: ContextId(0),
        scene: None,
    };
    let mut b = Side {
        client: ClientId(nexus_abi::service_id_from_name(b"gpud-context-b")),
        ctx: ContextId(0),
        scene: None,
    };
    a.ctx =
        dev.create_context(a.client, Priority::Realtime).map_err(|e| alloc::format!("a {e:?}"))?;
    b.ctx = match dev.create_context(b.client, Priority::High) {
        Ok(c) => c,
        Err(e) => {
            let _ = dev.destroy_context(a.client, a.ctx);
            return Err(alloc::format!("b {e:?}"));
        }
    };
    let mut extra = [None; 2];
    let r = drive(dev, display, scene, canvas, stale, &mut a, &mut b, &mut extra);
    // Torn down however the round went: the contexts, then their buffers.
    let t0 = now_ns();
    let gone_a = dev.destroy_context(a.client, a.ctx);
    let gone_b = dev.destroy_context(b.client, b.ctx);
    let cleanup_us = now_ns().saturating_sub(t0) / 1000;
    for side in [&a, &b] {
        if let Some(l) = &side.scene {
            l.release(dev, side.client, side.ctx);
        }
    }
    for bo in extra.iter().flatten() {
        let _ = dev.destroy_bo(b.client, *bo);
    }
    let mut round = r?;
    gone_a.and(gone_b).map_err(|e| alloc::format!("teardown {e:?}"))?;
    round.cleanup_us = cleanup_us;
    Ok((round, [a.ctx, b.ctx]))
}

/// The round's work between making and tearing down.
#[allow(clippy::too_many_arguments)]
fn drive(
    dev: &mut Device,
    display: &mut dyn Display,
    scene: &Scene<'_>,
    canvas: GpuCanvas,
    stale: Option<[ContextId; 2]>,
    a: &mut Side,
    b: &mut Side,
    extra: &mut [Option<BoId>; 2],
) -> Result<Round, alloc::string::String> {
    let target = scene.target().ok_or("scene without a target")?;
    let streams = render_streams(scene).ok_or("the bundle lacks one of the three render jobs")?;
    let setup = scene.render().ok_or("scene without the render trailer")?;
    let mut round = Round::default();
    // The first round's numbers, refused now their slots serve again.
    if let Some([old_a, old_b]) = stale {
        let batch = batch_at(target.va, WIDTH, HEIGHT, streams);
        for (client, old) in [(a.client, old_a), (b.client, old_b)] {
            if dev.submit(client, old, QueueKind::Render, &batch) != Err(DeviceError::NoSuchContext)
            {
                return Err(alloc::format!(
                    "the first round's context {:#x} was not refused",
                    old.0
                ));
            }
            round.refused += 1;
        }
    }
    // B's picture: a buffer of B's, cleaned once (no dirty line over the GPU's pixels).
    let b_target = dev
        .create_bo(b.client, PLANE_BYTES, CacheMode::CpuCached)
        .map_err(|e| alloc::format!("b's target {e:?}"))?;
    extra[0] = Some(b_target);
    dev.clean(b.client, b_target);
    if canvas.block != 0 {
        if let Some(bytes) = display.gpu_canvas_bytes(canvas.row) {
            nexus_abi::cache_clean(bytes, canvas.block);
        }
    }
    let plane = TargetMem::Plane { bus: canvas.bus };
    a.scene = Some(scene_load::load(dev, a.client, a.ctx, scene, plane, Texel::Checker)?);
    let own = TargetMem::Buffer(b_target);
    b.scene = Some(scene_load::load(dev, b.client, b.ctx, scene, own, Texel::CheckerSwapped)?);
    dev.render_setup(a.client, a.ctx, &setup).map_err(|e| alloc::format!("a setup {e:?}"))?;
    dev.render_setup(b.client, b.ctx, &setup).map_err(|e| alloc::format!("b setup {e:?}"))?;
    round.catalogues =
        (dev.catalogue(a.client, a.ctx).unwrap_or(0), dev.catalogue(b.client, b.ctx).unwrap_or(0));
    if round.catalogues.0 == round.catalogues.1 {
        return Err(alloc::string::String::from("one catalogue for two contexts"));
    }
    // Both passes in flight together.
    let batch = batch_at(target.va, WIDTH, HEIGHT, streams);
    let t0 = now_ns();
    let pa = dev
        .submit(a.client, a.ctx, QueueKind::Render, &batch)
        .map_err(|e| alloc::format!("a submit {e:?}"))?;
    let pb = dev
        .submit(b.client, b.ctx, QueueKind::Render, &batch)
        .map_err(|e| alloc::format!("b submit {e:?}"))?;
    let deadline = t0 + PASS_TIMEOUT_NS;
    if !dev.wait_point(pa, deadline) || !dev.wait_point(pb, deadline) {
        dev.serve_firmware();
        let wa = dev.render_words(a.client, a.ctx).unwrap_or((0, 0));
        let wb = dev.render_words(b.client, b.ctx).unwrap_or((0, 0));
        return Err(alloc::format!(
            "passes not done in {}us (a={pa:#x?} words={wa:?} b={pb:#x?} words={wb:?})",
            now_ns().saturating_sub(t0) / 1000
        ));
    }
    round.both_us = now_ns().saturating_sub(t0) / 1000;
    // Each picture its context's own: A's in the plane, B's in its buffer.
    let plane_bytes = display.gpu_canvas_bytes(canvas.row).ok_or("no cpu view of the plane")?;
    if canvas.block != 0 {
        nexus_abi::cache_flush(plane_bytes, canvas.block);
    }
    let (good_a, bad_a) = check(plane_bytes, &SAMPLES);
    if let Some((i, x, y, want, got)) = bad_a {
        return Err(alloc::format!("a sample {i} ({x},{y}) want {want:02x?} read {got:02x?}"));
    }
    let block = dev.gpu.mem.block();
    let b_bytes = dev.bo_bytes_mut(b.client, b_target).ok_or("no cpu view of b's target")?;
    if block != 0 {
        nexus_abi::cache_flush(b_bytes, block);
    }
    let (good_b, bad_b) = check(b_bytes, &SAMPLES_SWAPPED);
    if let Some((i, x, y, want, got)) = bad_b {
        return Err(alloc::format!("b sample {i} ({x},{y}) want {want:02x?} read {got:02x?}"));
    }
    round.samples = good_a + good_b;
    round.refused += refusals(dev, a, b, extra, b_target, target.va, streams)?;
    Ok(round)
}

/// Each context refused the other's buffers and addresses, by name; how many were.
fn refusals(
    dev: &mut Device,
    a: &Side,
    b: &Side,
    extra: &mut [Option<BoId>; 2],
    b_target: BoId,
    target_va: u64,
    streams: nexus_gfx::backend::v2::RenderStreams<'_>,
) -> Result<usize, alloc::string::String> {
    let a_bo = a.scene.as_ref().and_then(Loaded::a_buffer).ok_or("a's scene has no buffer")?;
    let page = |bo| Mapping::whole(bo, 4096, GpuVa(B_ONLY_VA), Access::ReadOnly);
    // An address only B binds.
    let b_page = dev
        .create_bo(b.client, 4096, CacheMode::GpuPrivate)
        .map_err(|e| alloc::format!("b's page {e:?}"))?;
    extra[1] = Some(b_page);
    dev.vm_bind(b.client, b.ctx, page(b_page)).map_err(|e| alloc::format!("b's page {e:?}"))?;
    let small = batch_at(B_ONLY_VA, 32, 32, streams);
    let full = batch_at(target_va, WIDTH, HEIGHT, streams);
    let asked = [
        // A's buffer in B's context, by B; by A, in B's context.
        dev.vm_bind(b.client, b.ctx, page(a_bo)) == Err(DeviceError::NotOwner),
        dev.vm_bind(a.client, b.ctx, page(a_bo)) == Err(DeviceError::NotOwner),
        // B unbinding A's target; A submitting in B's context.
        dev.vm_unbind(b.client, a.ctx, GpuVa(target_va)) == Err(DeviceError::NotOwner),
        dev.submit(a.client, b.ctx, QueueKind::Render, &full) == Err(DeviceError::NotOwner),
        // A pass of A's at the address only B binds.
        dev.submit(a.client, a.ctx, QueueKind::Render, &small)
            == Err(DeviceError::Vm(nexus_driverkit::gpu::VmError::Unbound)),
        // A buffer still bound is not destroyed — B's target, B's page.
        dev.destroy_bo(b.client, b_target) == Err(DeviceError::BufferBound),
        dev.destroy_bo(b.client, b_page) == Err(DeviceError::BufferBound),
        // A's buffer is not B's to destroy.
        dev.destroy_bo(b.client, a_bo) == Err(DeviceError::NotOwner),
        // A destroying B's context.
        dev.destroy_context(a.client, b.ctx) == Err(DeviceError::NotOwner),
    ];
    let refused = asked.iter().filter(|&&r| r).count();
    if refused != asked.len() {
        let which = asked.iter().position(|&r| !r).unwrap_or(0);
        return Err(alloc::format!("refusal {which} was not refused ({refused}/{})", asked.len()));
    }
    // The page goes before the context: an unbind and a destroy are the client's to ask.
    dev.vm_unbind(b.client, b.ctx, GpuVa(B_ONLY_VA)).map_err(|e| alloc::format!("{e:?}"))?;
    Ok(refused)
}

pub(super) fn batch_at(
    va: u64,
    width: u32,
    height: u32,
    streams: nexus_gfx::backend::v2::RenderStreams<'_>,
) -> Batch<'_> {
    Batch {
        target: RenderTarget { va: GpuVa(va), width, height },
        stream: Stream::Render(streams),
        waits: &[],
        signal: TimelinePoint(0),
    }
}
