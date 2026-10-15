// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The cost of a pass (TASK-0215 5c-1, RFC-0101 §8d). The first cycle took 5b's pass apart on
//! the board (measurement README, 5c-1): the GPU-cache policy is 2–4 % of it (the device maps
//! every buffer by the open driver's flags since), pixel merging is as small and unsafe by the
//! open driver's own word, the background drawn over the whole plane is 2.1 ms of a pass's
//! 2.5 ms fixed cost, and the fragment stage carries the per-pixel cost — 13 ns a pixel, seven
//! times the stock's opaque fill. This rung keeps two things on the board:
//!
//! - the gate: a pass limited to its quad's area — the back end's clip, the region clip, no
//!   background on empty tiles — draws the quad and leaves every pixel outside the area as it
//!   was: a sentinel in the quad's own tile, one far away, both kept, and the pass cheaper than
//!   the same pass over the whole plane (`gpud: gpu area ok (`);
//! - the pipeline gate (5c-2 P3c): the compositor's layer compiled from WGSL by our toolchain
//!   (`resources/gpu/pipelines`) — the untinted opaque variant and the tinted premultiplied one,
//!   each on a 1024² quad limited to its area: both samples right and the opaque layer no slower
//!   a pixel than the stock stack's opaque fill (1.8 ns): `gpud: gpu pipeline ok (`.
//!
//! Each pass lands in fresh lines of the rung's halves of the bundle's buffers' tails — no
//! buffer of its own: four of them filled gpud's 64 address-space regions and the display's
//! cursor layer found none (the second cycle's first boot). A program goes into the free tail
//! of the bundle's code arena; the pass names it through a copy of the fragment kick word and
//! its pass type through a copy of the draw's state block.

use nexus_driverkit::gpu::{Access, GpuVa};
use nexus_gfx::backend::gpu::csb::{isp, pbe};
use nexus_gfx::backend::gpu::fwif::{FragCmd, GeomCmd, RenderFeatures};
use nexus_gfx::backend::gpu::quads::{self, Bump, Quad, Template};
use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{
    Batch, ClientId, ContextId, GfxDevice, QueueKind, RenderStreams, RenderTarget, Stream,
    TimelinePoint,
};

use super::device_v2::Device;
use super::emit;
use super::render_template::{
    expected_bgr, opaque_bgr, pds_at, premultiplied_bgr, render_streams, write_bound, Captured,
    COST_TAILS, GENERAL_ARENA, PDS_ARENA, SAMPLE_FRACTION, STREAM_BUFFER, VERTEX_BUFFER,
};
use crate::backend::display::{Display, GpuCanvas};

const BUNDLE: &[u8] = include_bytes!("../../../../../../resources/gpu/scenes/render-quad.scene");
const PASS_TIMEOUT_NS: u64 = 2_000_000_000;
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const STRIDE: usize = 7680;
const TOLERANCE: u8 = 4;
/// The bundle's code arena (the code heap's second page) and its free tail, a page a program.
const CODE_HEAP_BASE: u64 = 0xe0_0000_0000;
const CODE_ARENA: u64 = 0xe0_0000_1000;
const CODE_TAIL: u32 = 0x8000;
/// The fragment kick word as captured: the base program at the code heap's offset 0x7b80,
/// five units of four temporaries (`resources/gpu/programs/README.md`); bits 2–31 the offset.
const KICK_CAPTURED: u64 = 0x0000_0028_0000_7b80;
const KICK_OFFSET: u64 = 0xffff_fffc;
/// The draw state block's third word is the ISP word: the pass type in bits 24–26.
const ISP_WORD: usize = 8;
const PASS_TYPE: u32 = 7 << 24;
const PUNCH_THROUGH: u32 = 2 << 24;
const TRANSLUCENT: u32 = 1 << 24;
/// The sentinels: one a pixel above the 16² quad in the quad's own tile (the back end's clip,
/// pixel-precise), one in the plane's first tile (the region clip).
const NEAR: (usize, usize) = (710, 283);
const FAR: (usize, usize) = (8, 8);
const SENTINEL: [u8; 4] = [0x3c, 0xa5, 0x5a, 0xff];
const WHITE: [f32; 4] = [1.0; 4];

/// A compositor pipeline compiled by the toolchain (`resources/gpu/pipelines`, 5c-2 P3c): its
/// program, the temporaries and pass type the compiler gave it (`tools/nxgpu/tests/pipelines.rs`
/// pins them until the pipeline library carries them), the tint its probe draws with.
struct Pipeline {
    name: &'static str,
    code: &'static [u8],
    groups: usize,
    temps: u8,
    pass_type: u32,
    tint: [f32; 4],
    premultiplied: bool,
}

const OPAQUE: u32 = 0;

const PIPELINES: [Pipeline; 2] = [
    Pipeline {
        name: "layer-opaque",
        code: include_bytes!("../../../../../../resources/gpu/pipelines/layer-opaque.usc"),
        groups: 3,
        temps: 1,
        pass_type: OPAQUE,
        tint: WHITE,
        premultiplied: false,
    },
    Pipeline {
        name: "layer-premultiplied",
        code: include_bytes!("../../../../../../resources/gpu/pipelines/layer-premultiplied.usc"),
        groups: 11,
        temps: 3,
        pass_type: TRANSLUCENT,
        tint: [0.5; 4],
        premultiplied: true,
    },
];

/// The stock stack's opaque textured fill, the opaque layer's bar: hundredths of a ns a pixel
/// (`docs/board/measurements/2026-10-10-gpu-stock-baseline/`).
const STOCK_OPAQUE_FILL_CNS: u64 = 180;
/// The pixels of the 1024² probe quad.
const PROBE_PIXELS: u64 = 1024 * 1024;

/// One pass's times from the kick: the whole, the geometry stage, the fragment stage.
struct Pass {
    us: u64,
    geom_us: u64,
    frag_us: u64,
    irqs: u32,
}

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// The quad of a size: 16² at (704, 284), 1024² at (448, 28).
fn quad(size: u32) -> Quad {
    let (x, y) = if size >= 1024 { (448.0, 28.0) } else { (704.0, 284.0) };
    Quad { x, y, w: size as f32, h: size as f32, tint: WHITE }
}

/// The sample point inside a quad.
fn sample_at(q: &Quad) -> (usize, usize) {
    ((q.x + SAMPLE_FRACTION * q.w) as usize, (q.y + SAMPLE_FRACTION * q.h) as usize)
}

/// The pixel at `(x, y)`, its cache block dropped first (the GPU wrote memory).
fn peek(display: &mut dyn Display, canvas: &GpuCanvas, (x, y): (usize, usize)) -> Option<[u8; 4]> {
    let bytes = display.gpu_canvas_bytes(canvas.row)?;
    let at = y * STRIDE + x * 4;
    let px = bytes.get(at..at + 4)?;
    if canvas.block != 0 {
        nexus_abi::cache_flush(px, canvas.block);
    }
    Some([px[0], px[1], px[2], px[3]])
}

/// Writes the sentinel at `(x, y)` and cleans its block (CPU → device).
fn poke(display: &mut dyn Display, canvas: &GpuCanvas, (x, y): (usize, usize)) -> bool {
    let Some(bytes) = display.gpu_canvas_bytes(canvas.row) else { return false };
    let at = y * STRIDE + x * 4;
    let Some(px) = bytes.get_mut(at..at + 4) else { return false };
    px.copy_from_slice(&SENTINEL);
    if canvas.block != 0 {
        nexus_abi::cache_clean(px, canvas.block);
    }
    true
}

/// Whether the sample of `q` reads `want`; the reading for the line otherwise.
fn sample(
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    q: &Quad,
    want: [u8; 3],
) -> Result<(), alloc::string::String> {
    match peek(display, canvas, sample_at(q)) {
        Some(px) if px.iter().zip(&want).all(|(p, w)| p.abs_diff(*w) <= TOLERANCE) => Ok(()),
        Some(px) => Err(alloc::format!(
            "bad(read={:02x}{:02x}{:02x} want={:02x}{:02x}{:02x})",
            px[0],
            px[1],
            px[2],
            want[0],
            want[1],
            want[2]
        )),
        None => Err(alloc::string::String::from("bad(outside)")),
    }
}

/// The three streams of a pass: the geometry naming the pass's control stream; the fragment
/// jobs with the back end's clip and no background on empty tiles when the pass is
/// area-limited.
fn streams_for(
    base: &RenderStreams<'_>,
    feats: RenderFeatures,
    stream_base: u64,
    q: &Quad,
    area: bool,
) -> Result<(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>, alloc::vec::Vec<u8>), &'static str> {
    let mut g = GeomCmd::from_stream(base.geom, feats, base.geom_flags, 0, Default::default())
        .map_err(|_| "geometry stream")?;
    g.regs.vdm_ctrl_stream_base = stream_base;
    let frag = |stream: &[u8], flags: u32| -> Result<alloc::vec::Vec<u8>, &'static str> {
        let mut f = FragCmd::from_stream(stream, feats, flags, 0).map_err(|_| "fragment stream")?;
        if area {
            let (x0, y0) = (q.x as u32, q.y as u32);
            let (x1, y1) = (x0 + q.w as u32 - 1, y0 + q.h as u32 - 1);
            f.regs.pbe_word[0] = pbe::with_min_x(f.regs.pbe_word[0], x0);
            f.regs.pbe_word[1] = pbe::with_clip(f.regs.pbe_word[1], y0, x1, y1);
            f.regs.isp_ctl &= !isp::CTL_PROCESS_EMPTY_TILES;
        }
        Ok(f.to_stream(feats))
    };
    Ok((
        g.to_stream(feats),
        frag(base.frag_pr, base.frag_pr_flags)?,
        frag(base.frag, base.frag_flags)?,
    ))
}

/// The rung's fixed context: the device's client and context, the bundle's streams, the target.
struct Rig<'a> {
    client: ClientId,
    ctx: ContextId,
    base: RenderStreams<'a>,
    target_va: u64,
}

/// One pass of `qs` (the first names the area): emit, write, kick, stamp the stages.
fn run(
    dev: &mut Device,
    rig: &Rig<'_>,
    t: &Template<'_>,
    place: &mut Bump,
    qs: &[Quad],
    area: bool,
) -> Result<Pass, alloc::string::String> {
    let q = qs.first().ok_or("no quad")?;
    let emitted = quads::emit(t, qs, place, false).map_err(|e| alloc::format!("emit {e:?}"))?;
    let (client, ctx) = (rig.client, rig.ctx);
    write_bound(dev, client, VERTEX_BUFFER, emitted.vertex_offset, &emitted.vertices)?;
    write_bound(dev, client, STREAM_BUFFER, emitted.stream_offset, &emitted.stream)?;
    for w in &emitted.general {
        write_bound(dev, client, GENERAL_ARENA, w.offset, &w.bytes)?;
    }
    for w in &emitted.pds {
        write_bound(dev, client, PDS_ARENA, pds_at(PDS_ARENA, w.offset), &w.bytes)?;
    }
    let feats = dev.gpu.fw.render;
    let (geom, frag_pr, frag) =
        streams_for(&rig.base, feats, emitted.vdm_ctrl_stream_base, q, area)
            .map_err(alloc::string::String::from)?;
    let batch = Batch {
        target: RenderTarget { va: GpuVa(rig.target_va), width: WIDTH, height: HEIGHT },
        stream: Stream::Render(RenderStreams {
            geom: &geom,
            frag_pr: &frag_pr,
            frag: &frag,
            ..rig.base
        }),
        waits: &[],
        signal: TimelinePoint(0),
    };
    let irqs_before = dev.gpu.irqs;
    let t0 = now_ns();
    dev.submit(client, ctx, QueueKind::Render, &batch)
        .map_err(|e| alloc::format!("submit {e:?}"))?;
    let Some(((gtl, gseq), (ftl, fseq))) = dev.render_progress() else {
        return Err(alloc::string::String::from("no render objects"));
    };
    let reached = |word: u32, point: u32| word.wrapping_sub(point) < 0x8000_0000;
    let mut t_geom = 0u64;
    let done = dev.gpu.wait_until(t0 + PASS_TIMEOUT_NS, |g| {
        if t_geom == 0 && reached(g.mem.read_u32(&gtl, 0), gseq) {
            t_geom = now_ns();
        }
        reached(g.mem.read_u32(&ftl, 0), fseq)
    });
    let t_end = now_ns();
    dev.gpu.drain_fwccb();
    let irqs = dev.gpu.irqs - irqs_before;
    if !done {
        return Err(alloc::format!(
            "pass lost after {}us (geom_us={} irqs={irqs})",
            t_end.saturating_sub(t0) / 1000,
            t_geom.saturating_sub(t0) / 1000
        ));
    }
    Ok(Pass {
        us: t_end.saturating_sub(t0) / 1000,
        geom_us: t_geom.saturating_sub(t0) / 1000,
        frag_us: t_end.saturating_sub(t_geom.max(t0)) / 1000,
        irqs,
    })
}

/// The gate: the 16² pass over the whole plane, the sentinels, the same pass limited to the
/// quad's area; the line.
fn gate(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    rig: &Rig<'_>,
    t: &Template<'_>,
    place: &mut Bump,
) -> Result<u64, alloc::string::String> {
    let q = quad(16);
    let full = run(dev, rig, t, place, &[q], false).map_err(|e| alloc::format!("full pass {e}"))?;
    if !poke(display, canvas, NEAR) || !poke(display, canvas, FAR) {
        return Err(alloc::string::String::from("no cpu view of the plane"));
    }
    let area = run(dev, rig, t, place, &[q], true).map_err(|e| alloc::format!("area pass {e}"))?;
    let inside = sample(display, canvas, &q, expected_bgr(WHITE));
    let kept = |px: Option<[u8; 4]>| if px == Some(SENTINEL) { "kept" } else { "overwritten" };
    let (near, far) = (kept(peek(display, canvas, NEAR)), kept(peek(display, canvas, FAR)));
    let line = alloc::format!(
        "(us={} full_us={} geom_us={} frag_us={} inside={} near={near} far={far})",
        area.us,
        full.us,
        area.geom_us,
        area.frag_us,
        inside.as_ref().map_or_else(|e| e.as_str(), |_| "ok")
    );
    if inside.is_ok() && near == "kept" && far == "kept" && area.us < full.us {
        emit(&alloc::format!("gpud: gpu area ok {line}"));
        Ok(area.us)
    } else {
        Err(line)
    }
}

/// The pipeline gate: each compiled pipeline at the code arena's tail, named by a copy of the
/// kick word with its temporaries, its pass type in a copy of the state block; a 1024² pass
/// limited to its area, its sample, its cost a pixel beyond the limited 16² pass (`area_us`).
fn pipelines(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    rig: &Rig<'_>,
    t: &Template<'_>,
    place: &mut Bump,
    area_us: u64,
) {
    let fail = |what: &str| emit(&alloc::format!("gpud: FAIL gpu pipeline ({what})"));
    let kick = t
        .shader_program
        .get(..8)
        .map(|w| u64::from_le_bytes([w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7]]));
    let isp =
        t.ppp26.get(ISP_WORD..ISP_WORD + 4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
    let (Some(KICK_CAPTURED), Some(isp)) = (kick, isp) else {
        return fail("the bundle's kick word is not the captured one");
    };
    if isp & PASS_TYPE != PUNCH_THROUGH {
        return fail("the bundle's pass type is not the captured one");
    }
    let mut cns = [0u64; 2];
    for (i, p) in PIPELINES.iter().enumerate() {
        let at = CODE_TAIL + i as u32 * 0x1000;
        if let Err(e) = write_bound(dev, rig.client, CODE_ARENA, at, p.code) {
            return fail(&alloc::format!("{} write {e}", p.name));
        }
        let offset = (CODE_ARENA + u64::from(at) - CODE_HEAP_BASE) & KICK_OFFSET;
        let mut shader = alloc::vec::Vec::from(t.shader_program);
        shader[..8].copy_from_slice(&((u64::from(p.temps) << 35) | offset).to_le_bytes());
        let mut ppp = alloc::vec::Vec::from(t.ppp26);
        ppp[ISP_WORD..ISP_WORD + 4]
            .copy_from_slice(&((isp & !PASS_TYPE) | p.pass_type).to_le_bytes());
        let tp = Template { shader_program: &shader, ppp26: &ppp, ..*t };
        let q = Quad { tint: p.tint, ..quad(1024) };
        let pass = match run(dev, rig, &tp, place, &[q], true) {
            Ok(pass) => pass,
            Err(e) => return fail(&alloc::format!("{} {e}", p.name)),
        };
        let want = if p.premultiplied { premultiplied_bgr(p.tint) } else { opaque_bgr(p.tint) };
        let reading = sample(display, canvas, &q, want);
        cns[i] = pass.us.saturating_sub(area_us) * 100_000 / PROBE_PIXELS;
        emit(&alloc::format!(
            "gpud: gpu cost (program={} groups={} pass={} size=1024 us={} geom_us={} frag_us={} irqs={} ns_px={}.{:02} sample={})",
            p.name,
            p.groups,
            if p.pass_type == OPAQUE { "opaque" } else { "translucent" },
            pass.us,
            pass.geom_us,
            pass.frag_us,
            pass.irqs,
            cns[i] / 100,
            cns[i] % 100,
            reading.as_ref().map_or_else(|e| e.as_str(), |_| "ok")
        ));
        if let Err(e) = reading {
            return fail(&alloc::format!("{} sample {e}", p.name));
        }
    }
    let line = alloc::format!(
        "(opaque_ns_px={}.{:02} premultiplied_ns_px={}.{:02} samples=2/2 bar={}.{:02})",
        cns[0] / 100,
        cns[0] % 100,
        cns[1] / 100,
        cns[1] % 100,
        STOCK_OPAQUE_FILL_CNS / 100,
        STOCK_OPAQUE_FILL_CNS % 100
    );
    if cns[0] <= STOCK_OPAQUE_FILL_CNS {
        emit(&alloc::format!("gpud: gpu pipeline ok {line}"));
    } else {
        fail(&alloc::format!("the opaque layer above the stock fill {line}"));
    }
}

/// The rung: after `gpu quads ok`, with the render rung's bindings and objects.
#[inline(never)]
pub(crate) fn rung(dev: &mut Device, display: &mut dyn Display) {
    let fail = |what: &str| emit(&alloc::format!("gpud: FAIL gpu area ({what})"));
    let core = CoreId(dev.gpu.win.core_id());
    let Ok(scene) = Scene::parse(BUNDLE, core) else {
        return fail("scene refused");
    };
    let (Some(target), Some(base)) = (scene.target(), render_streams(&scene)) else {
        return fail("scene without a target or the three render jobs");
    };
    let client = ClientId(nexus_abi::service_id_from_name(b"gpud"));
    let ctx = ContextId(1);
    let captured = match Captured::read(dev, client) {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    let template = captured.template(GENERAL_ARENA, VERTEX_BUFFER, STREAM_BUFFER);
    let Some(canvas) = display.gpu_canvas() else {
        return fail("no plane of the ring free");
    };
    let rebind = dev
        .vm_unbind(client, ctx, GpuVa(target.va))
        .and_then(|_| dev.adopt_plane(client, canvas.bus, target.len as usize))
        .and_then(|bo| dev.vm_bind(client, ctx, bo, GpuVa(target.va), Access::ReadWrite));
    if let Err(e) = rebind {
        display.gpu_canvas_discard(canvas.row);
        return fail(&alloc::format!("rebind target {e:?}"));
    }
    // No dirty line of the plane may be written back over the GPU's pixels.
    if canvas.block != 0 {
        if let Some(bytes) = display.gpu_canvas_bytes(canvas.row) {
            nexus_abi::cache_clean(bytes, canvas.block);
        }
    }
    let mut place: Bump = COST_TAILS;
    let rig = Rig { client, ctx, base, target_va: target.va };
    match gate(dev, display, &canvas, &rig, &template, &mut place) {
        Ok(area_us) => pipelines(dev, display, &canvas, &rig, &template, &mut place, area_us),
        Err(e) => fail(&e),
    }
    display.gpu_canvas_discard(canvas.row);
    // gpud's heap never frees: what the GPU rungs left of it.
    let (start, now, end) = nexus_service_entry::os::heap_cursor();
    emit(&alloc::format!("gpud: gpu heap (used={} of={})", now - start, end - start));
}
