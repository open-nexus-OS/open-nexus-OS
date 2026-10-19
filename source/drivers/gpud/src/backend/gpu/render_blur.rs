// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The glass layer's backdrop blur on the board (TASK-0215 5c-2 P5d-5): the chain a frame runs
//! for a glass layer — the horizontal pass from the plane's full-resolution backdrop into half
//! resolution, the vertical pass with the saturation, the rounded layer sampling the result back
//! over the glass — each a library pipeline on a pass limited to its quad, each reading its
//! texture through our own descriptor (`texstate`): the plane as a stride texture, then the
//! vertical result's rectangle of it. The CPU writes the backdrop (red stripes, a green ramp
//! down, a blue one across), so the expected pixels are the virgl path's full-resolution
//! gaussian of it (σ = R/2, taps out to ±R) with the CPU path's saturation, computed here. The
//! intermediates live in rows of the same plane below the picture: a pass names its area, so
//! nothing else of the plane changes. (In the frame they are the context's GPU-only buffers.)
//! Since 5c-3 P4b the chain runs at every radius windowd's glass asks — 20 in the pipeline
//! gate's line, 8, 30 and 40 in `gpud: gpu blur radii ok (…)` — each read held to its radius's
//! deviation on the host and the texture unit's three levels.

use alloc::format;
use alloc::string::String;

use nexus_gfx::backend::gpu::pipelines::Placed;
use nexus_gfx::backend::gpu::quads::{Bump, Need, Placer, Quad, Template};
use nexus_gfx::backend::gpu::texstate::{descriptor_bytes, format, swizzle, Image, Sampler};

use super::device_v2::Device;
use super::render_cost::{cost, named, peek, run, Pass, Rig, HEIGHT, STRIDE, WIDTH};
use super::render_template::{clear_bgr, write_bound, COST_TAILS, GENERAL_ARENA};
use crate::backend::display::{Display, GpuCanvas};

/// The glass: a 256x192 layer at (512, 96), corner radius 16; windowd's glass blur (saturation
/// 140 %).
const GLASS: (u32, u32, u32, u32) = (512, 96, 256, 192);
const CORNER: f32 = 16.0;
const SATURATION: f32 = 1.4;
/// The backdrop the CPU writes: the glass and 48 pixels around it (the widest taps reach 40
/// pixels, a bilinear fetch one more).
const MARGIN: u32 = 48;
/// The radii the chain runs at besides 20 (the pipeline gate's): windowd's other glass levels.
const OTHER_RADII: [u32; 3] = [8, 30, 40];
/// The intermediates, below the picture: the horizontal result at (512, 640) — the glass's
/// half-resolution columns, its rows and R/2 more above and below for the vertical taps —, the
/// vertical one at (800, 640).
const ACROSS: (u32, u32) = (512, 640);
const DOWN: (u32, u32) = (800, 640);
/// The reads: inside the glass (its centre, near its left and top edges, a point low right) and
/// its corner pixel, which the rounded layer leaves out — the pass's clear there, as for the
/// rounded probe (a pass clears its area: loading what the plane held is the frame's, 5c-3).
const READS: [(u32, u32); 5] = [(640, 192), (520, 192), (640, 100), (700, 260), (512, 96)];
/// Levels a read may differ from the full-resolution formula at `radius`: the reduced chain's
/// deviation on the host (`tools/nxgpu/tests/blur.rs`: seven levels at radius 8 at half
/// resolution, ten at 20, 30 and 40 at quarter) and the texture unit's filter fractions (three).
fn tolerance(radius: u32) -> u8 {
    3 + match radius {
        8 => 7,
        // Quarter resolution from radius 20 on (P6): ten levels on the host at the checker's
        // edges.
        _ => 10,
    }
}

/// What the chain cost and read.
pub(super) struct Blur {
    pub across: Pass,
    pub down: Pass,
    pub layer: Pass,
    /// Each pass's output pixels.
    pub pixels: [u64; 3],
    pub reads: usize,
    /// The largest difference of a read from its formula, in levels.
    pub deviation: u8,
}

/// The backdrop's colour (R, G, B) at a plane pixel of its rectangle.
fn backdrop(x: u32, y: u32) -> [u8; 3] {
    let (x0, y0) = (GLASS.0 - MARGIN, GLASS.1 - MARGIN);
    let red = if (x / 16) % 2 == 0 { 220 } else { 30 };
    let across = (x - x0) * 255 / (GLASS.2 + 2 * MARGIN - 1);
    [red, (y - y0) as u8, across as u8]
}

/// The virgl path's kernel: 2R + 1 weights 2^(k·i²), k = −1/(2σ² ln 2), σ = R/2, normalised.
fn kernel(radius: u32) -> alloc::vec::Vec<f32> {
    let sigma = (radius as f32 / 2.0).max(0.5);
    let k = -1.0 / (2.0 * sigma * sigma * core::f32::consts::LN_2);
    let mut w: alloc::vec::Vec<f32> = (0..=2 * radius)
        .map(|i| {
            let d = i as f32 - radius as f32;
            libm::exp2f(k * d * d)
        })
        .collect();
    let total: f32 = w.iter().sum();
    w.iter_mut().for_each(|x| *x /= total);
    w
}

/// The expected bytes (B, G, R) at `(x, y)` inside the glass: the full-resolution gaussian of
/// the backdrop at `radius`, saturated.
fn expected((x, y): (u32, u32), radius: u32) -> [u8; 3] {
    let w = kernel(radius);
    let mut c = [0.0f32; 3];
    for (j, wj) in w.iter().enumerate() {
        for (i, wi) in w.iter().enumerate() {
            let p = backdrop(x + i as u32 - radius, y + j as u32 - radius);
            for (acc, v) in c.iter_mut().zip(p) {
                *acc += wi * wj * f32::from(v);
            }
        }
    }
    let grey = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    let s = |v: f32| libm::roundf(grey + (v - grey) * SATURATION).clamp(0.0, 255.0) as u8;
    [s(c[2]), s(c[1]), s(c[0])]
}

/// The backdrop written into the plane (memory order B, G, R, A), each row cleaned.
fn write_backdrop(display: &mut dyn Display, canvas: &GpuCanvas) -> bool {
    let Some(bytes) = display.gpu_canvas_bytes(canvas.row) else { return false };
    let (x0, y0) = ((GLASS.0 - MARGIN) as usize, GLASS.1 - MARGIN);
    let w = (GLASS.2 + 2 * MARGIN) as usize;
    for y in y0..GLASS.1 + GLASS.3 + MARGIN {
        let at = y as usize * STRIDE + x0 * 4;
        let Some(row) = bytes.get_mut(at..at + 4 * w) else { return false };
        for (i, px) in row.chunks_exact_mut(4).enumerate() {
            let [r, g, b] = backdrop((x0 + i) as u32, y);
            px.copy_from_slice(&[b, g, r, 0xff]);
        }
        if canvas.block != 0 {
            nexus_abi::cache_clean(row, canvas.block);
        }
    }
    true
}

/// `image`'s descriptor written into fresh lines of the general arena: its address.
pub(super) fn descriptor(
    dev: &mut Device,
    rig: &Rig<'_>,
    place: &mut dyn Placer,
    image: &Image,
) -> Result<u64, String> {
    let bytes = descriptor_bytes(image, &Sampler::BILINEAR_CLAMP)
        .map_err(|e| format!("descriptor {}", e.0))?;
    let off = place.place(0, Need::Descriptor).ok_or("no line for a descriptor")?;
    write_bound(dev, rig.client, rig.ctx, GENERAL_ARENA, off, &bytes)?;
    Ok(GENERAL_ARENA + u64::from(off))
}

/// The plane as a texture from `(x, y)` on, `w` × `h`: blue, green, red, alpha words.
pub(super) fn plane(rig: &Rig<'_>, (x, y): (u32, u32), w: u32, h: u32) -> Image {
    Image {
        address: rig.target.va + u64::from(y) * STRIDE as u64 + u64::from(x) * 4,
        width: w,
        height: h,
        stride: WIDTH,
        format: format::U8U8U8U8,
        swizzle: swizzle::BGRA,
    }
}

/// The chain at `radius` over the CPU's backdrop with the library's `across`, `down` and rounded
/// `layer` pipelines, its reads against the formula.
#[allow(clippy::too_many_arguments)]
pub(super) fn chain(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    rig: &Rig<'_>,
    t: &Template<'_>,
    place: &mut Bump,
    radius: u32,
    [across, down, layer]: [&Placed; 3],
) -> Result<Blur, String> {
    if !write_backdrop(display, canvas) {
        return Err(String::from("no cpu view of the plane"));
    }
    let (gx, gy, gw, gh) = GLASS;
    let (fw, fh) = (WIDTH as f32, HEIGHT as f32);
    // The chain's resolution (P6: half below radius 20, quarter from it on) and its taps.
    let s = nexus_gfx::frame::blur_scale(radius);
    let (taps, sf) = (radius / s, s as f32);
    let (hw, hh, vw, vh) = (gw / s, gh / s + 2 * taps, gw / s, gh / s);
    let whole = descriptor(dev, rig, place, &plane(rig, (0, 0), WIDTH, HEIGHT))?;
    let on_plane = Template { texture_va: whole, ..*t };
    // Horizontal: a reduced pixel's centre is the corner of the four backdrop texels it
    // averages, `scale · taps` full-resolution rows above the glass on; a tap `scale` texels
    // across.
    let halo = s * taps;
    let source =
        [sf * hw as f32 / fw, sf * hh as f32 / fh, gx as f32 / fw, (gy - halo) as f32 / fh];
    let q = Quad::blur(
        ACROSS.0 as f32,
        ACROSS.1 as f32,
        hw as f32,
        hh as f32,
        source,
        [sf / fw, 0.0],
        1.0,
    );
    let across_pass = run(dev, rig, &on_plane, across, place, &[q], true)?;
    // Vertical: the horizontal result's texel under the output pixel, `taps` rows down its halo;
    // a tap one row down; the saturation.
    let source =
        [vw as f32 / fw, vh as f32 / fh, ACROSS.0 as f32 / fw, (ACROSS.1 + taps) as f32 / fh];
    let q = Quad::blur(
        DOWN.0 as f32,
        DOWN.1 as f32,
        vw as f32,
        vh as f32,
        source,
        [0.0, 1.0 / fh],
        SATURATION,
    );
    let down_pass = run(dev, rig, &on_plane, down, place, &[q], true)?;
    // The glass: the rounded layer over the vertical result's rectangle, sampled back up.
    let result = descriptor(dev, rig, place, &plane(rig, DOWN, vw, vh))?;
    let q = Quad::layer(gx as f32, gy as f32, gw as f32, gh as f32, [1.0; 4], CORNER);
    let layer_pass =
        run(dev, rig, &Template { texture_va: result, ..*t }, layer, place, &[q], true)?;
    let mut deviation = 0;
    for (i, xy) in READS.iter().enumerate() {
        let want = if i + 1 == READS.len() { clear_bgr() } else { expected(*xy, radius) };
        let px = peek(display, canvas, (xy.0 as usize, xy.1 as usize)).ok_or("bad(outside)")?;
        let d = px.iter().zip(&want).map(|(p, w)| p.abs_diff(*w)).max().unwrap_or(0);
        if d > tolerance(radius) {
            return Err(format!(
                "bad(at={},{} read={:02x}{:02x}{:02x} want={:02x}{:02x}{:02x})",
                xy.0, xy.1, px[0], px[1], px[2], want[0], want[1], want[2]
            ));
        }
        deviation = deviation.max(d);
    }
    Ok(Blur {
        across: across_pass,
        down: down_pass,
        layer: layer_pass,
        pixels: [u64::from(hw * hh), u64::from(vw * vh), u64::from(gw * gh)],
        reads: READS.len(),
        deviation,
    })
}

/// The radii rung's line: `gpud: gpu blur radii ok (…)`, or its FAIL form.
#[allow(clippy::too_many_arguments)]
pub(super) fn radii_rung(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    rig: &Rig<'_>,
    t: &Template<'_>,
    place: &mut Bump,
    layer: &Placed,
    area_us: u64,
) {
    super::emit(&match radii(dev, display, canvas, rig, t, place, layer, area_us) {
        Ok(radii) => format!("gpud: gpu blur radii ok {radii}"),
        Err(e) => format!("gpud: FAIL gpu blur radii ({e})"),
    });
}

/// The chain at windowd's other radii (5c-3 P4b): each radius's two passes from the library and
/// the rounded layer sampling them back, every read against the radius's formula, each pass's
/// cost line. The line's part per radius: `r<radius>=<deviation>/<horizontal ns a pixel>/<vertical
/// ns a pixel>`.
#[allow(clippy::too_many_arguments)]
fn radii(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    rig: &Rig<'_>,
    t: &Template<'_>,
    place: &mut Bump,
    layer: &Placed,
    area_us: u64,
) -> Result<String, String> {
    let ns = |c: u64| format!("{}.{:02}", c / 100, c % 100);
    let ok = Ok(());
    let mut line = String::from("(");
    for radius in OTHER_RADII {
        // Every pass before was waited: the cost rung's tails are free again (three chains
        // overran them on the board, cycle 29).
        *place = COST_TAILS;
        let (h, v) = (format!("blur-horizontal-{radius}"), format!("blur-vertical-{radius}"));
        let (across, down) = (named(dev, rig, &h)?, named(dev, rig, &v)?);
        let b = chain(dev, display, canvas, rig, t, place, radius, [&across, &down, layer])
            .map_err(|e| format!("radius {radius} {e}"))?;
        let h_cns = cost(&h, &across, &b.across, b.pixels[0], area_us, &ok);
        let v_cns = cost(&v, &down, &b.down, b.pixels[1], area_us, &ok);
        line.push_str(&format!("r{radius}={}/{}/{} ", b.deviation, ns(h_cns), ns(v_cns)));
    }
    line.push_str(&format!(
        "tolerance={}/{}/{})",
        tolerance(OTHER_RADII[0]),
        tolerance(OTHER_RADII[1]),
        tolerance(OTHER_RADII[2])
    ));
    Ok(line)
}
