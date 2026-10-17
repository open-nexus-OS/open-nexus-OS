// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A fault stays in its context, on the board (TASK-0215 5c-3 P2c, RFC-0101 §6). Two contexts
//! of two clients load the render bundle at the same addresses, as the contexts rung does — A's
//! target a plane of the ring, F's a buffer of F's own — and each draws once: both pictures
//! right. Then F unbinds the buffer its passes write their region headers into (the write
//! fault the board measured in 5a cycle 3), and F's next pass is submitted with A's beside it:
//! F's work writes an address F's space no longer maps — A maps the same address to memory of
//! its own — so the GPU's memory unit faults in F's work alone. The device serves the
//! firmware's messages while it waits: the reset is held against F by F's page catalogue, F is
//! lost — its point counts as reached, its submits, binds, resets and setups are refused
//! `ContextLost` — and the free lists the reset cut off are rebuilt and answered, so the
//! firmware runs on. A's pass beside the fault lands right and A draws once more; a fresh
//! context of a third client draws right after the fault. Every pass's samples are marked
//! before it runs, so a picture read is the pass's own. F, A and C are destroyed, every page
//! and buffer back, nothing in quarantine: `gpud: gpu containment ok (`.

use alloc::string::String;

use nexus_display_proto::layout::PLANE_BYTES;
use nexus_driverkit::gpu::{Access, CacheMode, GpuVa};
use nexus_gfx::backend::gpu::fwif::HwrtArgs;
use nexus_gfx::backend::gpu::scene::{RenderSetup, Scene};
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{
    Batch, BoId, ClientId, ContextId, DeviceError, GfxDevice, Mapping, Priority, QueueKind,
    TimelinePoint,
};

use super::device_v2::Device;
use super::emit;
use super::render_contexts::batch_at;
use super::render_quad::{check, SAMPLES, STRIDE};
use super::render_template::render_streams;
use super::scene_load::{self, Loaded, TargetMem, Texel};
use crate::backend::display::{Display, GpuCanvas};

const BUNDLE: &[u8] = include_bytes!("../../../../../../resources/gpu/scenes/render-quad.scene");
const PASS_TIMEOUT_NS: u64 = 2_000_000_000;
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
/// What a sample holds before its pass writes it: no colour of the picture.
const SENTINEL: [u8; 4] = [0x5a, 0xa5, 0x5a, 0xff];
/// Samples read: A before, F before, A beside the fault, A after it, C after it.
const SAMPLES_ALL: usize = 5 * SAMPLES.len();
/// Refusals asked of the lost context: submit, bind, reset, setup.
const REFUSALS: usize = 4;

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// A context of the rung: its client, its number, its scene, its own target buffer.
struct Side {
    client: ClientId,
    ctx: ContextId,
    scene: Option<Loaded>,
    target: Option<BoId>,
}

impl Side {
    fn new(name: &[u8]) -> Self {
        Self {
            client: ClientId(nexus_abi::service_id_from_name(name)),
            ctx: ContextId(0),
            scene: None,
            target: None,
        }
    }
}

/// What the rung measured.
#[derive(Default)]
struct Outcome {
    samples: usize,
    refused: usize,
    /// F's page catalogue, the address its reset named, the region headers' range.
    catalogue: u64,
    fault: u64,
    headers: (u64, u64),
    /// From F's faulting submit until both points were reached.
    fault_us: u64,
    /// F's timeline words after the fault against its last points: `(geometry, fragment)`.
    f_words: (u32, u32),
    f_points: (u32, u32),
    /// F torn down: how long it took; whether C was made in F's slot (the lost context's slot
    /// serving again).
    f_teardown_us: u64,
    c_slot_of_f: bool,
}

/// The rung: after the contexts rung, on a device that holds nothing.
#[inline(never)]
pub(crate) fn rung(dev: &mut Device, display: &mut dyn Display) {
    let fail = |what: &str| emit(&alloc::format!("gpud: FAIL gpu containment ({what})"));
    let baseline = dev.gpu.baseline_free;
    let h = dev.holdings();
    if h.live + h.lost + h.quarantined != 0 || h.bos != 0 || h.backing_free != baseline {
        return fail(&alloc::format!(
            "the device was not empty (live={} lost={} quarantined={} bos={} backing={}/{baseline})",
            h.live,
            h.lost,
            h.quarantined,
            h.bos,
            h.backing_free
        ));
    }
    let Ok(scene) = Scene::parse(BUNDLE, CoreId(dev.gpu.win.core_id())) else {
        return fail("scene refused");
    };
    let Some(canvas) = display.gpu_canvas() else {
        return fail("no plane of the ring free");
    };
    let before = dev.recovered();
    let mut sides =
        [Side::new(b"gpud"), Side::new(b"gpud-context-f"), Side::new(b"gpud-context-c")];
    let outcome = contain(dev, display, &scene, canvas, &mut sides);
    // Torn down however the rung went: the contexts, then their buffers.
    let t0 = now_ns();
    let mut gone = Ok(());
    for side in &mut sides {
        gone = gone.and(retire(dev, side));
    }
    let teardown_us = now_ns().saturating_sub(t0) / 1000;
    display.gpu_canvas_discard(canvas.row);
    let out = match outcome {
        Ok(out) => out,
        Err(e) => return fail(&e),
    };
    if let Err(e) = gone {
        return fail(&alloc::format!("teardown {e:?}"));
    }
    let rec = dev.recovered();
    let h = dev.holdings();
    let lost = rec.lost - before.lost;
    let unattributed = rec.unattributed - before.unattributed;
    let unknown = rec.unknown_lists - before.unknown_lists;
    let line = alloc::format!(
        "(contexts=3 samples={}/{SAMPLES_ALL} refused={}/{REFUSALS} lost={lost} pc_f={:#x} fault={:#x} headers={:#x}+{:#x} fault_us={} f_words={}/{} of {}/{} f_teardown_us={} c_slot_of_f={} resets={} innocent={} reconstructions={} rebuilt={} unknown_lists={unknown} grows={} unattributed={unattributed} teardown_us={teardown_us} backing={}/{baseline} bos={} quarantined={} cleanup_busy={})",
        out.samples,
        out.refused,
        out.catalogue,
        out.fault,
        out.headers.0,
        out.headers.1,
        out.fault_us,
        out.f_words.0,
        out.f_words.1,
        out.f_points.0,
        out.f_points.1,
        out.f_teardown_us,
        out.c_slot_of_f,
        rec.resets - before.resets,
        rec.innocent - before.innocent,
        rec.reconstructions - before.reconstructions,
        rec.rebuilt - before.rebuilt,
        rec.grows - before.grows,
        h.backing_free,
        h.bos,
        h.quarantined,
        dev.gpu.cleanup_busy
    );
    let whole = out.samples == SAMPLES_ALL && out.refused == REFUSALS && lost == 1;
    let clean = unattributed == 0 && unknown == 0;
    let back = h.live + h.lost + h.quarantined == 0 && h.bos == 0 && h.backing_free == baseline;
    if whole && clean && back {
        emit(&alloc::format!("gpud: gpu containment ok {line}"));
    } else {
        fail(&line);
    }
}

/// The rung's work between making and tearing down.
fn contain(
    dev: &mut Device,
    display: &mut dyn Display,
    scene: &Scene<'_>,
    canvas: GpuCanvas,
    sides: &mut [Side; 3],
) -> Result<Outcome, String> {
    let [a, f, c] = sides;
    let target = scene.target().ok_or("scene without a target")?;
    let streams = render_streams(scene).ok_or("the bundle lacks one of the three render jobs")?;
    let setup = scene.render().ok_or("scene without the render trailer")?;
    let batch = batch_at(target.va, WIDTH, HEIGHT, streams);
    let mut out = Outcome::default();
    // A and F made, loaded, set up; each draws once.
    a.ctx = dev.create_context(a.client, Priority::Realtime).map_err(|e| err("a", e))?;
    f.ctx = dev.create_context(f.client, Priority::Normal).map_err(|e| err("f", e))?;
    prepare(dev, a, scene, &setup, TargetMem::Plane { bus: canvas.bus })?;
    let f_target = own_target(dev, f)?;
    prepare(dev, f, scene, &setup, TargetMem::Buffer(f_target))?;
    out.catalogue = dev.catalogue(f.client, f.ctx).unwrap_or(0);
    mark(plane(display, canvas)?, canvas.block);
    let pa = submit(dev, a, &batch)?;
    let pf = submit(dev, f, &batch)?;
    wait(dev, &[pa, pf], "the passes before the fault")?;
    out.samples += read(plane(display, canvas)?, canvas.block, "a before")?;
    let block = dev.gpu.mem.block();
    out.samples += read(buffer(dev, f, f_target)?, block, "f before")?;
    // F's region headers unbound from F's space; F's next pass beside A's.
    out.headers = headers(scene, &setup)?;
    unbind_range(dev, f, out.headers)?;
    mark(plane(display, canvas)?, canvas.block);
    let t0 = now_ns();
    let pf2 = submit(dev, f, &batch)?;
    let pa2 = submit(dev, a, &batch)?;
    wait(dev, &[pf2, pa2], "the passes of the fault")?;
    out.fault_us = now_ns().saturating_sub(t0) / 1000;
    dev.serve_firmware();
    let progress = dev.render_progress(f.client, f.ctx).ok_or("f's render objects are gone")?;
    out.f_points = (progress.0 .1, progress.1 .1);
    out.f_words = dev.render_words(f.client, f.ctx).unwrap_or((0, 0));
    if !dev.lost(f.client, f.ctx) {
        return Err(alloc::format!(
            "f's pass was not held against f (words={:?} of {:?})",
            out.f_words,
            out.f_points
        ));
    }
    if dev.lost(a.client, a.ctx) {
        return Err(String::from("a was lost beside f"));
    }
    match dev.recovered().last {
        Some((ctx, pc, fault)) if ctx == f.ctx && pc == out.catalogue => out.fault = fault,
        other => return Err(alloc::format!("the loss was not f's by its catalogue ({other:x?})")),
    }
    out.samples += read(plane(display, canvas)?, canvas.block, "a beside the fault")?;
    out.refused = refusals(dev, f, &batch, &setup);
    // A draws on.
    mark(plane(display, canvas)?, canvas.block);
    let pa3 = submit(dev, a, &batch)?;
    wait(dev, &[pa3], "a's pass after the fault")?;
    out.samples += read(plane(display, canvas)?, canvas.block, "a after")?;
    // F goes — the firmware lets go of a lost context's objects too — then a fresh context of
    // a third client draws.
    let f_slot = f.ctx.0 & 0xff;
    let t1 = now_ns();
    retire(dev, f).map_err(|e| err("f's teardown", e))?;
    out.f_teardown_us = now_ns().saturating_sub(t1) / 1000;
    let h = dev.holdings();
    if h.lost + h.quarantined != 0 {
        return Err(alloc::format!(
            "f's teardown left lost={} quarantined={}",
            h.lost,
            h.quarantined
        ));
    }
    c.ctx = dev.create_context(c.client, Priority::High).map_err(|e| err("c", e))?;
    out.c_slot_of_f = c.ctx.0 & 0xff == f_slot;
    let c_target = own_target(dev, c)?;
    prepare(dev, c, scene, &setup, TargetMem::Buffer(c_target))?;
    let pc = submit(dev, c, &batch)?;
    wait(dev, &[pc], "c's pass")?;
    out.samples += read(buffer(dev, c, c_target)?, block, "c")?;
    Ok(out)
}

fn err(who: &str, e: DeviceError) -> String {
    alloc::format!("{who} {e:?}")
}

/// The side's context destroyed (live or lost), then its scene's buffers and its own target;
/// the side holds nothing afterwards. The context's refusal is returned once its buffers went
/// as far as they can.
fn retire(dev: &mut Device, side: &mut Side) -> Result<(), DeviceError> {
    let gone = if side.ctx.0 == 0 { Ok(()) } else { dev.destroy_context(side.client, side.ctx) };
    if let Some(l) = side.scene.take() {
        l.release(dev, side.client, side.ctx);
    }
    if let Some(bo) = side.target.take() {
        let _ = dev.destroy_bo(side.client, bo);
    }
    side.ctx = ContextId(0);
    gone
}

/// A target buffer of the side's own, its samples marked, cleaned once.
fn own_target(dev: &mut Device, side: &mut Side) -> Result<BoId, String> {
    let bo = dev
        .create_bo(side.client, PLANE_BYTES, CacheMode::CpuCached)
        .map_err(|e| err("target", e))?;
    side.target = Some(bo);
    let block = dev.gpu.mem.block();
    let bytes = dev.bo_bytes_mut(side.client, bo).ok_or("no cpu view of a target")?;
    mark(bytes, block);
    dev.clean(side.client, bo);
    Ok(bo)
}

/// The scene loaded into the side's context and its render objects set up.
fn prepare(
    dev: &mut Device,
    side: &mut Side,
    scene: &Scene<'_>,
    setup: &RenderSetup<'_>,
    target: TargetMem,
) -> Result<(), String> {
    side.scene = Some(scene_load::load(dev, side.client, side.ctx, scene, target, Texel::Checker)?);
    dev.render_setup(side.client, side.ctx, setup).map_err(|e| err("setup", e))
}

fn submit(dev: &mut Device, side: &Side, batch: &Batch<'_>) -> Result<TimelinePoint, String> {
    dev.submit(side.client, side.ctx, QueueKind::Render, batch).map_err(|e| err("submit", e))
}

/// Waits for every point of `points` (a lost context's counts as reached).
fn wait(dev: &mut Device, points: &[TimelinePoint], what: &str) -> Result<(), String> {
    let deadline = now_ns() + PASS_TIMEOUT_NS;
    match points.iter().find(|&&p| !dev.wait_point(p, deadline)) {
        Some(p) => Err(alloc::format!("{what}: point {:#x} not reached", p.0)),
        None => Ok(()),
    }
}

/// The range of the scene's buffers holding the render-target data's region headers: the
/// memory a pass's geometry writes into.
fn headers(scene: &Scene<'_>, setup: &RenderSetup<'_>) -> Result<(u64, u64), String> {
    let args = HwrtArgs::parse(setup.hwrt_args).map_err(|_| "the dataset's arguments refused")?;
    let at = args.rt_data[0].region_header_dev_addr;
    let buf = scene
        .buffers()
        .find(|b| b.va <= at && at < b.va + b.len)
        .ok_or("no buffer holds the region headers")?;
    let second = args.rt_data[1].region_header_dev_addr;
    if second < buf.va || second >= buf.va + buf.len {
        return Err(String::from("the two data's region headers lie in two buffers"));
    }
    Ok((buf.va, buf.len))
}

/// The side's binding of `range` undone (the client's own unbind: the device unmaps and
/// flushes, so the GPU no longer reaches the memory through this context).
fn unbind_range(dev: &mut Device, side: &Side, range: (u64, u64)) -> Result<(), String> {
    dev.vm_unbind(side.client, side.ctx, GpuVa(range.0)).map_err(|e| err("unbind", e))
}

/// The lost context refused by name for new work: its submit, a bind, a reset, a setup.
fn refusals(dev: &mut Device, f: &Side, batch: &Batch<'_>, setup: &RenderSetup<'_>) -> usize {
    let lost = Err(DeviceError::ContextLost);
    let page = f.target.map(|bo| Mapping::whole(bo, 4096, GpuVa(0x90_0000_0000), Access::ReadOnly));
    [
        dev.submit(f.client, f.ctx, QueueKind::Render, batch).map(|_| ()) == lost,
        page.is_some_and(|m| dev.vm_bind(f.client, f.ctx, m) == lost),
        dev.reset_context(f.client, f.ctx) == lost,
        dev.render_setup(f.client, f.ctx, setup) == lost,
    ]
    .iter()
    .filter(|&&r| r)
    .count()
}

fn plane(display: &mut dyn Display, canvas: GpuCanvas) -> Result<&mut [u8], String> {
    display.gpu_canvas_bytes(canvas.row).ok_or_else(|| String::from("no cpu view of the plane"))
}

fn buffer<'a>(dev: &'a mut Device, side: &Side, bo: BoId) -> Result<&'a mut [u8], String> {
    dev.bo_bytes_mut(side.client, bo).ok_or_else(|| String::from("no cpu view of a target"))
}

/// The samples of a picture marked and cleaned (CPU → device): a pass that lands writes each.
fn mark(bytes: &mut [u8], block: usize) {
    for (x, y, _) in SAMPLES {
        let at = y as usize * STRIDE + x as usize * 4;
        if let Some(px) = bytes.get_mut(at..at + SENTINEL.len()) {
            px.copy_from_slice(&SENTINEL);
            if block != 0 {
                nexus_abi::cache_clean(px, block);
            }
        }
    }
}

/// The samples of a picture read, their lines flushed first (the GPU wrote them).
fn read(bytes: &mut [u8], block: usize, who: &str) -> Result<usize, String> {
    if block != 0 {
        for (x, y, _) in SAMPLES {
            let at = y as usize * STRIDE + x as usize * 4;
            if let Some(px) = bytes.get(at..at + 4) {
                nexus_abi::cache_flush(px, block);
            }
        }
    }
    let (good, bad) = check(bytes, &SAMPLES);
    match bad {
        Some((i, x, y, want, got)) => {
            Err(alloc::format!("{who}: sample {i} ({x},{y}) want {want:02x?} read {got:02x?}"))
        }
        None => Ok(good),
    }
}
