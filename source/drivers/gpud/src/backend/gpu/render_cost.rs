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
//! - the pipeline gate (5c-2 P3c, P4d, P5): every pipeline of the context's library (the library
//!   rung placed it), each on a pass limited to its quad — the four layers (opaque, premultiplied,
//!   straight, rounded: its interior and the corner pixel it leaves out) on the 1024² quad, a
//!   window's drop shadow (under the window knocked out, below it the shadow), the glass blur's
//!   chain over a backdrop the CPU wrote (`render_blur`: two passes and the rounded layer, read
//!   against the full-resolution formula) — every read right and the opaque layer no slower a
//!   pixel than the stock stack's opaque fill (1.8 ns): `gpud: gpu pipeline ok (`.
//!
//! Each pass lands in fresh lines of the rung's halves of the bundle's buffers' tails — no
//! buffer of its own: four of them filled gpud's 64 address-space regions and the display's
//! cursor layer found none (the second cycle's first boot). A pass names its pipeline by id;
//! the pipeline's setup sets the pass type and the setup programs.

use nexus_driverkit::gpu::{Access, GpuVa};
use nexus_gfx::backend::gpu::csb::{isp, pbe};
use nexus_gfx::backend::gpu::fwif::{FragCmd, GeomCmd, RenderFeatures};
use nexus_gfx::backend::gpu::pipelines::Placed;
use nexus_gfx::backend::gpu::quads::{self, Bump, Quad, Template};
use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{
    Batch, ClientId, ContextId, GfxDevice, QueueKind, RenderStreams, RenderTarget, Stream,
    TimelinePoint,
};
use nexus_gfx::shader::rogue::reference::PASS_OPAQUE;

use super::device_v2::Device;
use super::emit;
use super::render_blur;
use super::render_template::{
    clear_bgr, colour_over_clear_bgr, opaque_bgr, pds_at, premultiplied_bgr, render_streams,
    straight_bgr, write_bound, Captured, COST_TAILS, GENERAL_ARENA, PDS_ARENA, SAMPLE_FRACTION,
    STREAM_BUFFER, VERTEX_BUFFER,
};
use crate::backend::display::{Display, GpuCanvas};

const BUNDLE: &[u8] = include_bytes!("../../../../../../resources/gpu/scenes/render-quad.scene");
const PASS_TIMEOUT_NS: u64 = 2_000_000_000;
/// The plane: 1920x1080 pixels of four bytes.
pub(super) const WIDTH: u32 = 1920;
pub(super) const HEIGHT: u32 = 1080;
pub(super) const STRIDE: usize = 7680;
const TOLERANCE: u8 = 4;
/// The sentinels: one a pixel above the 16² quad in the quad's own tile (the back end's clip,
/// pixel-precise), one in the plane's first tile (the region clip).
const NEAR: (usize, usize) = (710, 283);
const FAR: (usize, usize) = (8, 8);
const SENTINEL: [u8; 4] = [0x3c, 0xa5, 0x5a, 0xff];
const WHITE: [f32; 4] = [1.0; 4];

/// The library pipeline the area gate draws with (`resources/gpu/pipelines/compositor.toml`).
const AREA_LAYER: &str = "layer-premultiplied";
/// The glass blur's two passes (radius 20, windowd's glass).
const BLUR_ACROSS: &str = "blur-horizontal-20";
const BLUR_DOWN: &str = "blur-vertical-20";
/// The straight layers' tint (opacity 0.75), the rounded one's corner radius.
const STRAIGHT_TINT: [f32; 4] = [1.0, 1.0, 1.0, 0.75];
const ROUNDED_RADIUS: f32 = 64.0;
/// The shadow probe: a 512x384 window at (704, 284) with corner radius 12, the windows' shadow —
/// blur 18, offset (0, 5), black at alpha 90/255 (windowd's glass window).
const SHAPE: (f32, f32, f32, f32, f32) = (704.0, 284.0, 512.0, 384.0, 12.0);
const SHADOW_BLUR: f32 = 18.0;
const SHADOW_OFFSET: [f32; 2] = [0.0, 5.0];
const SHADOW_COLOUR: [f32; 4] = [0.0, 0.0, 0.0, 90.0 / 255.0];

/// The stock stack's opaque textured fill, the opaque layer's bar: hundredths of a ns a pixel
/// (`docs/board/measurements/2026-10-10-gpu-stock-baseline/`).
const STOCK_OPAQUE_FILL_CNS: u64 = 180;

/// One pass's times from the kick: the whole, the geometry stage, the fragment stage.
pub(super) struct Pass {
    pub us: u64,
    pub geom_us: u64,
    pub frag_us: u64,
    pub irqs: u32,
}

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// The quad of a size: 16² at (704, 284), 1024² at (448, 28).
fn quad(size: u32) -> Quad {
    let (x, y) = if size >= 1024 { (448.0, 28.0) } else { (704.0, 284.0) };
    Quad::layer(x, y, size as f32, size as f32, WHITE, 0.0)
}

/// The sample point inside a quad.
fn sample_at(q: &Quad) -> (usize, usize) {
    ((q.x + SAMPLE_FRACTION * q.w) as usize, (q.y + SAMPLE_FRACTION * q.h) as usize)
}

/// The pixel at `(x, y)`, its cache block dropped first (the GPU wrote memory).
pub(super) fn peek(
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    (x, y): (usize, usize),
) -> Option<[u8; 4]> {
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
    read(display, canvas, sample_at(q), want)
}

/// Whether the pixel at `xy` reads `want`; the reading for the line otherwise.
fn read(
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    xy: (usize, usize),
    want: [u8; 3],
) -> Result<(), alloc::string::String> {
    match peek(display, canvas, xy) {
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
pub(super) struct Rig<'a> {
    pub client: ClientId,
    pub ctx: ContextId,
    pub base: RenderStreams<'a>,
    pub target_va: u64,
}

/// One pass of `qs` (the first names the area): emit, write, kick, stamp the stages.
pub(super) fn run(
    dev: &mut Device,
    rig: &Rig<'_>,
    t: &Template<'_>,
    pipeline: &Placed,
    place: &mut Bump,
    qs: &[Quad],
    area: bool,
) -> Result<Pass, alloc::string::String> {
    let q = qs.first().ok_or("no quad")?;
    let emitted = quads::emit(t, pipeline, qs, place).map_err(|e| alloc::format!("emit {e:?}"))?;
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
    pipeline: &Placed,
    place: &mut Bump,
) -> Result<u64, alloc::string::String> {
    let q = quad(16);
    let full = run(dev, rig, t, pipeline, place, &[q], false)
        .map_err(|e| alloc::format!("full pass {e}"))?;
    if !poke(display, canvas, NEAR) || !poke(display, canvas, FAR) {
        return Err(alloc::string::String::from("no cpu view of the plane"));
    }
    let area = run(dev, rig, t, pipeline, place, &[q], true)
        .map_err(|e| alloc::format!("area pass {e}"))?;
    let inside = sample(display, canvas, &q, premultiplied_bgr(WHITE));
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

/// The shadow probe's alpha at the pixel centre `y` on the vertical axis through its shape (no
/// square root there): the distance outside the shifted shape falling off over the blur,
/// squared, knocked out across the unshifted shape (`resources/gpu/shaders/shadow.wgsl`).
fn shadow_alpha_on_axis(y: f32) -> f32 {
    let (top, h, r) = (SHAPE.1, SHAPE.3, SHAPE.4);
    let (inner, centre) = (h / 2.0 - r, top + h / 2.0);
    let outside = |c: f32| (libm::fabsf(y - c) - inner).max(0.0) - r;
    let ramp = (1.0 - outside(centre + SHADOW_OFFSET[1]) / SHADOW_BLUR).clamp(0.0, 1.0);
    SHADOW_COLOUR[3] * ramp * ramp * outside(centre).clamp(0.0, 1.0)
}

/// A probe of the pipeline gate: a library pipeline on one quad, the points read back with the
/// bytes they must hold.
struct Probe {
    name: &'static str,
    quad: Quad,
    reads: alloc::vec::Vec<((usize, usize), [u8; 3])>,
}

/// The gate's probes: the four layers on the 1024² quad, the shadow of a window.
fn probes() -> [Probe; 5] {
    let big = quad(1024);
    let layer = |tint: [f32; 4], r: f32| Quad::layer(big.x, big.y, big.w, big.h, tint, r);
    let (opaque, premultiplied) = (layer(WHITE, 0.0), layer([0.5; 4], 0.0));
    let (straight, rounded) = (layer(STRAIGHT_TINT, 0.0), layer(STRAIGHT_TINT, ROUNDED_RADIUS));
    let (x, y, w, h, r) = SHAPE;
    let shadow = Quad::shadow(x, y, w, h, r, SHADOW_BLUR, SHADOW_OFFSET, SHADOW_COLOUR);
    // Under the window's centre (knocked out) and six pixels below its bottom edge.
    let axis = (x + w / 2.0) as usize;
    let (under, below) = ((y + h / 2.0) as usize, (y + h) as usize + 6);
    let below_alpha = shadow_alpha_on_axis(below as f32 + 0.5);
    [
        Probe {
            name: "layer-opaque",
            quad: opaque,
            reads: alloc::vec![(sample_at(&opaque), opaque_bgr(WHITE))],
        },
        Probe {
            name: "layer-premultiplied",
            quad: premultiplied,
            reads: alloc::vec![(sample_at(&premultiplied), premultiplied_bgr([0.5; 4]))],
        },
        Probe {
            name: "layer-straight",
            quad: straight,
            reads: alloc::vec![(sample_at(&straight), straight_bgr(STRAIGHT_TINT))],
        },
        // The interior, and the corner pixel the rounded rectangle leaves out.
        Probe {
            name: "layer-rounded",
            quad: rounded,
            reads: alloc::vec![
                (sample_at(&rounded), straight_bgr(STRAIGHT_TINT)),
                ((rounded.x as usize + 1, rounded.y as usize + 1), clear_bgr()),
            ],
        },
        Probe {
            name: "shadow",
            quad: shadow,
            reads: alloc::vec![
                ((axis, under), clear_bgr()),
                ((axis, below), colour_over_clear_bgr(SHADOW_COLOUR, below_alpha)),
            ],
        },
    ]
}

/// A pass's cost a pixel beyond the limited 16² pass (`area_us`), in hundredths of a ns, and its
/// line.
fn cost(
    name: &str,
    p: &Placed,
    pass: &Pass,
    pixels: u64,
    area_us: u64,
    reading: &Result<(), alloc::string::String>,
) -> u64 {
    let cns = pass.us.saturating_sub(area_us) * 100_000 / pixels.max(1);
    emit(&alloc::format!(
        "gpud: gpu cost (program={name} id={} groups={} pass={} pixels={pixels} us={} geom_us={} frag_us={} irqs={} ns_px={}.{:02} reads={})",
        p.id,
        p.groups,
        if p.pass_type == PASS_OPAQUE { "opaque" } else { "translucent" },
        pass.us,
        pass.geom_us,
        pass.frag_us,
        pass.irqs,
        cns / 100,
        cns % 100,
        reading.as_ref().map_or_else(|e| e.as_str(), |_| "ok")
    ));
    cns
}

/// The library pipeline of `name`.
fn named(dev: &Device, name: &str) -> Result<Placed, alloc::string::String> {
    match dev.pipelines.id_of(name).map(|id| dev.pipelines.get(id)) {
        Some(Ok(p)) => Ok(p.clone()),
        _ => Err(alloc::format!("{name} is not in the context's library")),
    }
}

/// The pipeline gate: each probe's library pipeline on a pass limited to its quad, its reads,
/// its cost a pixel beyond the limited 16² pass (`area_us`) over its quad; then the glass blur's
/// chain (`render_blur`), its reads against the full-resolution formula.
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
    let probes = probes();
    let mut cns = [0u64; 5];
    let mut reads = 0;
    for (i, probe) in probes.iter().enumerate() {
        let p = match named(dev, probe.name) {
            Ok(p) => p,
            Err(e) => return fail(&e),
        };
        let pass = match run(dev, rig, t, &p, place, &[probe.quad], true) {
            Ok(pass) => pass,
            Err(e) => return fail(&alloc::format!("{} {e}", probe.name)),
        };
        let mut reading = Ok(());
        for (xy, want) in &probe.reads {
            if reading.is_ok() {
                reading = read(display, canvas, *xy, *want);
            }
        }
        let pixels = (probe.quad.w * probe.quad.h) as u64;
        cns[i] = cost(probe.name, &p, &pass, pixels, area_us, &reading);
        if let Err(e) = reading {
            return fail(&alloc::format!("{} {e}", probe.name));
        }
        reads += probe.reads.len();
    }
    let chain = [BLUR_ACROSS, BLUR_DOWN, "layer-rounded"].map(|name| named(dev, name));
    let [Ok(across), Ok(down), Ok(rounded)] = chain else {
        let e = chain.into_iter().find_map(Result::err).unwrap_or_default();
        return fail(&e);
    };
    let blur =
        match render_blur::chain(dev, display, canvas, rig, t, place, [&across, &down, &rounded]) {
            Ok(b) => b,
            Err(e) => return fail(&alloc::format!("blur {e}")),
        };
    let ok = Ok(());
    let blur_cns = [
        cost(BLUR_ACROSS, &across, &blur.across, blur.pixels[0], area_us, &ok),
        cost(BLUR_DOWN, &down, &blur.down, blur.pixels[1], area_us, &ok),
        cost("layer-rounded", &rounded, &blur.layer, blur.pixels[2], area_us, &ok),
    ];
    reads += blur.reads;
    let ns = |c: u64| alloc::format!("{}.{:02}", c / 100, c % 100);
    let line = alloc::format!(
        "(opaque_ns_px={} premultiplied_ns_px={} straight_ns_px={} rounded_ns_px={} shadow_ns_px={} blur_h_ns_px={} blur_v_ns_px={} blur_layer_ns_px={} blur_dev={} samples={reads}/{reads} bar={})",
        ns(cns[0]),
        ns(cns[1]),
        ns(cns[2]),
        ns(cns[3]),
        ns(cns[4]),
        ns(blur_cns[0]),
        ns(blur_cns[1]),
        ns(blur_cns[2]),
        blur.deviation,
        ns(STOCK_OPAQUE_FILL_CNS)
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
    let area_layer = match dev.pipelines.id_of(AREA_LAYER).map(|id| dev.pipelines.get(id)) {
        Some(Ok(p)) => p.clone(),
        _ => {
            display.gpu_canvas_discard(canvas.row);
            return fail("the library's premultiplied layer is not in the context");
        }
    };
    let mut place: Bump = COST_TAILS;
    let rig = Rig { client, ctx, base, target_va: target.va };
    match gate(dev, display, &canvas, &rig, &template, &area_layer, &mut place) {
        Ok(area_us) => pipelines(dev, display, &canvas, &rig, &template, &mut place, area_us),
        Err(e) => fail(&e),
    }
    display.gpu_canvas_discard(canvas.row);
    // gpud's heap never frees: what the GPU rungs left of it.
    let (start, now, end) = nexus_service_entry::os::heap_cursor();
    emit(&alloc::format!("gpud: gpu heap (used={} of={})", now - start, end - start));
}
