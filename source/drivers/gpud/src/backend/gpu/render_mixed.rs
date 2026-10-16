// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The mixed pass (TASK-0215 5c-3 P1): what a frame needs of a pass, on the board.
//!
//! - **Draws of different pipelines, pass types and textures in one pass** — the opaque layer over
//!   the checker, a straight layer over a colour block the CPU wrote, a rounded layer over the
//!   checker again: the pass type changes at the second draw (its block re-states the ISP words),
//!   each state load names its own texture.
//! - **A target per pass** — the same pass into two targets, each through its own copy of the
//!   captured end-of-tile program: the plane at its own address, and 540 rows down.
//! - **The pass memory as rings** — 48 passes of one to three draws through small rings, each
//!   draw with its own tint, every sample read back after its pass; each pass is reached before
//!   the next begins, so every pass writes the lines the pass before read (`reused=`): a line the
//!   GPU still held would show an earlier pass's tint (`stale=`), and gpud's heap does not move
//!   (`heap_bytes=`).

use alloc::format;
use alloc::string::String;

use nexus_gfx::backend::gpu::pipelines::Placed;
use nexus_gfx::backend::gpu::quads::{Arena, Bump, Draw, Need, Placer, Quad, Rings, Template};
use nexus_gfx::backend::v2::{GfxDevice, TimelinePoint};

use super::device_v2::Device;
use super::emit;
use super::render_blur::{descriptor, plane};
use super::render_cost::{named, read, run_draws, sample_at, Pass, Rig, STRIDE};
use super::render_template::{
    clear_bgr, colour_over_clear_bgr, opaque_bgr, premultiplied_bgr, straight_bgr, Captured,
    Target, Targets, TARGET_PDS,
};
use crate::backend::display::{Display, GpuCanvas};

/// The three draws (x, y, w, h): side by side over the pass's clear, inside its area.
const OPAQUE: (f32, f32, f32, f32) = (64.0, 64.0, 512.0, 256.0);
const STRAIGHT: (f32, f32, f32, f32) = (664.0, 64.0, 256.0, 128.0);
const ROUNDED: (f32, f32, f32, f32) = (964.0, 64.0, 256.0, 256.0);
const RADIUS: f32 = 64.0;
const STRAIGHT_TINT: [f32; 4] = [1.0, 1.0, 1.0, 0.75];
/// The pass's area (inclusive), in its target.
const AREA: (u32, u32, u32, u32) = (64, 64, 1219, 319);
/// The second target: the plane 540 rows down, 540 rows high.
const SECOND_ROW: u32 = 540;
/// The colour block the straight layer samples: a 64² block at (1792, 960) of the plane, its
/// colour (R, G, B) opaque.
const BLOCK: (u32, u32) = (1792, 960);
const BLOCK_SIDE: u32 = 64;
const BLOCK_RGB: [u8; 3] = [200, 120, 40];
/// The ring passes: `RING_PASSES` passes of one to three 16² draws at (1700 + 24 j, 400).
const RING_PASSES: usize = 48;
const RING_AT: (f32, f32) = (1700.0, 400.0);
/// The rings: the tails the quads and cost rungs drew in (every line already read by a finished
/// pass), cut small so they wrap every few passes; the colour block's descriptor in a line of
/// its own after the general ring.
const GENERAL_RING: Arena = Arena { base: 0x1_0000, len: 0x800 };
const PDS_RING: Arena = Arena { base: TARGET_PDS.1, len: 0x400 };
const STREAM_RING: Arena = Arena { base: 0x100, len: 0x100 };
const VERTEX_RING: Arena = Arena { base: 0x200, len: 0x200 };
const DESCRIPTOR_LINE: Bump = Bump {
    general_free: 0x1_0800,
    general_len: 0x1_0900,
    pds_free: 0,
    pds_len: 0,
    stream_free: 0,
    stream_len: 0,
    vertex_free: 0,
    vertex_len: 0,
};

/// The rings, counting the passes whose stream landed on lines an earlier pass's stream used
/// (below the stream ring's high-water mark).
struct Watched {
    rings: Rings,
    high_water: u32,
    reused: u32,
}

impl Placer for Watched {
    fn place(&mut self, draw: usize, need: Need) -> Option<u32> {
        let at = self.rings.place(draw, need)?;
        if let Need::Stream(n) = need {
            self.reused += u32::from(at < self.high_water);
            self.high_water = self.high_water.max(at + n);
        }
        Some(at)
    }
}

fn quad((x, y, w, h): (f32, f32, f32, f32), tint: [f32; 4], radius: f32) -> Quad {
    Quad::layer(x, y, w, h, tint, radius)
}

/// The colour block written into the plane (memory order B, G, R, A), each row cleaned.
fn write_block(display: &mut dyn Display, canvas: &GpuCanvas) -> bool {
    let Some(bytes) = display.gpu_canvas_bytes(canvas.row) else { return false };
    let [r, g, b] = BLOCK_RGB;
    for y in BLOCK.1..BLOCK.1 + BLOCK_SIDE {
        let at = y as usize * STRIDE + BLOCK.0 as usize * 4;
        let Some(row) = bytes.get_mut(at..at + 4 * BLOCK_SIDE as usize) else { return false };
        for px in row.chunks_exact_mut(4) {
            px.copy_from_slice(&[b, g, r, 0xff]);
        }
        if canvas.block != 0 {
            nexus_abi::cache_clean(row, canvas.block);
        }
    }
    true
}

/// One pass through the rings: begun, emitted, submitted at its point, retired once reached.
#[allow(clippy::too_many_arguments)]
fn pass<'p>(
    dev: &mut Device,
    rig: &Rig<'_>,
    t: &Template<'_>,
    rings: &mut Watched,
    n: usize,
    draw: &dyn Fn(usize) -> Draw<'p>,
    area: (u32, u32, u32, u32),
    target: Target,
) -> Result<Pass, String> {
    rings.rings.begin();
    match run_draws(dev, rig, t, n, draw, rings, Some(area), target) {
        Ok(p) => {
            rings.rings.submitted(p.point.0).map_err(|e| format!("rings {e:?}"))?;
            rings.rings.retire(&|point| dev.reached(TimelinePoint(point)));
            Ok(p)
        }
        Err(e) => {
            rings.rings.abandon();
            Err(e)
        }
    }
}

/// The rung: after the pipeline gate, on its plane and with its context.
pub(super) fn rung(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    rig: &Rig<'_>,
    t: &Template<'_>,
    captured: &Captured,
) {
    match mixed(dev, display, canvas, rig, t, captured) {
        Ok(line) => emit(&format!("gpud: gpu mixed ok {line}")),
        Err(e) => emit(&format!("gpud: FAIL gpu mixed ({e})")),
    }
}

fn mixed(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    rig: &Rig<'_>,
    t: &Template<'_>,
    captured: &Captured,
) -> Result<String, String> {
    let names = ["layer-opaque", "layer-straight", "layer-rounded", "layer-premultiplied"];
    let [opaque, straight, rounded, premultiplied] = names.map(|n| named(dev, rig, n));
    let (opaque, straight, rounded, premultiplied): (Placed, Placed, Placed, Placed) =
        (opaque?, straight?, rounded?, premultiplied?);
    if !write_block(display, canvas) {
        return Err(String::from("no cpu view of the plane"));
    }
    let block = descriptor(
        dev,
        rig,
        &mut DESCRIPTOR_LINE.clone(),
        &plane(rig, BLOCK, BLOCK_SIDE, BLOCK_SIDE),
    )?;
    let mut targets = Targets::new();
    let first = targets.at(dev, (rig.client, rig.ctx), captured, rig.target.va, rig.target.rows)?;
    let second_va = rig.target.va + u64::from(SECOND_ROW) * STRIDE as u64;
    let second_rows = rig.target.rows - SECOND_ROW;
    let second = targets.at(dev, (rig.client, rig.ctx), captured, second_va, second_rows)?;
    let mut rings = Watched {
        rings: Rings::new(GENERAL_RING, PDS_RING, STREAM_RING, VERTEX_RING),
        high_water: 0,
        reused: 0,
    };
    // The mixed pass into each target, its four reads.
    let draws = [
        Draw { pipeline: &opaque, quad: quad(OPAQUE, [1.0; 4], 0.0), texture_va: t.texture_va },
        Draw { pipeline: &straight, quad: quad(STRAIGHT, STRAIGHT_TINT, 0.0), texture_va: block },
        Draw {
            pipeline: &rounded,
            quad: quad(ROUNDED, STRAIGHT_TINT, RADIUS),
            texture_va: t.texture_va,
        },
    ];
    let colour = BLOCK_RGB.map(|c| f32::from(c) / 255.0);
    let block_over_clear = colour_over_clear_bgr([colour[0], colour[1], colour[2], 1.0], 0.75);
    let mut mixed_us = 0;
    // Each mixed pass's stages: the cost the pixel gate does not predict, taken apart.
    let mut stages = [(0u64, 0u64); 2];
    let mut reads = 0;
    for (k, (target, row)) in
        [(first, 0usize), (second, SECOND_ROW as usize)].into_iter().enumerate()
    {
        let p = pass(dev, rig, t, &mut rings, draws.len(), &|i| draws[i], AREA, target)
            .map_err(|e| format!("mixed pass at row {row}: {e}"))?;
        mixed_us += p.us;
        stages[k] = (p.geom_us, p.frag_us);
        let at = |xy: (usize, usize)| (xy.0, xy.1 + row);
        let (o, s, r) = (draws[0].quad, draws[1].quad, draws[2].quad);
        let corner = (r.x as usize + 1, r.y as usize + 1);
        for (xy, want) in [
            (sample_at(&o), opaque_bgr([1.0; 4])),
            (sample_at(&s), block_over_clear),
            (sample_at(&r), straight_bgr(STRAIGHT_TINT)),
            (corner, clear_bgr()),
        ] {
            read(display, canvas, at(xy), want).map_err(|e| format!("row {row} {xy:?}: {e}"))?;
            reads += 1;
        }
    }
    // The ring passes: each draw its own tint, every sample read after its pass.
    let heap_before = nexus_service_entry::os::heap_cursor().1;
    let mut ring_reads = 0;
    for k in 0..RING_PASSES {
        let n = k % 3 + 1;
        let tint = |j: usize| {
            let v = |m: usize, o: usize| ((k * m + j * 29 + o) % 200 + 40) as f32 / 255.0;
            [v(37, 0), v(91, 64), v(53, 128), 1.0]
        };
        let q = |j: usize| {
            Quad::layer(RING_AT.0 + 24.0 * j as f32, RING_AT.1, 16.0, 16.0, tint(j), 0.0)
        };
        let draw =
            |j: usize| Draw { pipeline: &premultiplied, quad: q(j), texture_va: t.texture_va };
        let (x0, y0) = (RING_AT.0 as u32, RING_AT.1 as u32);
        let area = (x0, y0, x0 + 24 * (n as u32 - 1) + 15, y0 + 15);
        pass(dev, rig, t, &mut rings, n, &draw, area, first)
            .map_err(|e| format!("ring pass {k}: {e}"))?;
        for j in 0..n {
            read(display, canvas, sample_at(&q(j)), premultiplied_bgr(tint(j)))
                .map_err(|e| format!("stale=1 ring pass {k} draw {j}: {e}"))?;
            ring_reads += 1;
        }
    }
    let heap_bytes = nexus_service_entry::os::heap_cursor().1.saturating_sub(heap_before);
    Ok(format!(
        "(draws={} targets=2 reads={reads}/{reads} mixed_us={mixed_us} geom_us={}/{} frag_us={}/{} ring_passes={RING_PASSES} ring_reads={ring_reads}/{ring_reads} reused={} stale=0 heap_bytes={heap_bytes})",
        draws.len(),
        stages[0].0,
        stages[1].0,
        stages[0].1,
        stages[1].1,
        rings.reused
    ))
}
