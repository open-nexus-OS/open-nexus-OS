// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A draw cut to its pass's area (TASK-0351 P5, ADR-0094: the planner's mathematics,
//! every executor's cut). A pass's area reaches the board's hardware only as the pixel back
//! end's clip: a full-screen quad kept for a pointer's one-pixel pass is still binned to every
//! tile and shaded whole — the board's hover frame cost 12.7–13.5 ms a pass in every cycle of
//! 2026-11-03 against the stock's 0.64 ms for a 16x16 frame, and 6–9 ms a pass whatever the
//! area. [`clip_draw`] cuts a kept draw to the rectangle the area and its quad share: the quad
//! covers the part, its push constants keep the whole shape (`Quad::part_of`, the corner's
//! distance and the shadow's falloff unchanged), and its texture rectangle moves with it when
//! the draw shows its texels one to one — a blur draw is already cut by its chain, a band spread
//! over a larger layer and a quad off the pixel grid are kept whole. Pure and allocation-free.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/frame_clip.rs` (the cut draw shades the same distance and samples the
//!   same texels at every pixel it keeps; a spread draw is refused the cut)
//! INVARIANTS: a pixel inside the pass area gets what the whole draw gave it; a draw outside the
//!   area is `None`; a draw inside it is itself.

use super::board::{DrawSpec, Pipe, Tex};
use super::chain::PassArea;
use crate::backend::gpu::quads::Quad;

/// The rectangle `q` and `area` (inclusive corners) share, as `(x, y, w, h)`; `None` when they
/// do not meet.
fn meet(q: &Quad, (ax0, ay0, ax1, ay1): PassArea) -> Option<(f32, f32, f32, f32)> {
    let (x0, y0) = (q.x.max(ax0 as f32), q.y.max(ay0 as f32));
    let (x1, y1) = ((q.x + q.w).min(ax1 as f32 + 1.0), (q.y + q.h).min(ay1 as f32 + 1.0));
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
}

/// Whether `q` lies on the pixel grid: its texels can be named as a rectangle.
fn on_the_grid(q: &Quad) -> bool {
    // No `fract` without the standard library: a whole number survives the round trip.
    [q.x, q.y, q.w, q.h].iter().all(|v| *v >= 0.0 && *v == (*v as u32) as f32)
}

/// `tex` moved by `(dx, dy)` and sized `(w, h)` when `q` shows it texel for texel; `None` when
/// the band is spread over the quad (its texels are not the quad's pixels).
fn cut_tex(tex: Tex, q: &Quad, (dx, dy, w, h): (u32, u32, u32, u32)) -> Option<Tex> {
    let one_to_one = |tw: u32, th: u32| (tw, th) == (q.w as u32, q.h as u32);
    match tex {
        Tex::None => Some(Tex::None),
        Tex::Layout { x, row, w: tw, h: th } if one_to_one(tw, th) => {
            Some(Tex::Layout { x: x + dx, row: row + dy, w, h })
        }
        Tex::Cache { x, row, w: tw, h: th } if one_to_one(tw, th) => {
            Some(Tex::Cache { x: x + dx, row: row + dy, w, h })
        }
        Tex::Buffer { source, slot, x, row, w: tw, h: th } if one_to_one(tw, th) => {
            Some(Tex::Buffer { source, slot, x: x + dx, row: row + dy, w, h })
        }
        Tex::Run { slot, x, row, w: tw, h: th } if one_to_one(tw, th) => {
            Some(Tex::Run { slot, x: x + dx, row: row + dy, w, h })
        }
        _ => None,
    }
}

/// A cache texture drawn at an exact integer scale `k` (a glass back: its rows at the blur's
/// resolution, its quad the glass) cut to the part `(dx, dy, w, h)` of its quad: the part snapped
/// outward to whole texels from the quad's origin, grown by one texel — the bilinear apron, so
/// every sample inside the part reads the neighbours the whole draw read — and held inside the
/// quad; the texture the same texels (TASK-0351 P13: a 1-px hover drew the whole 2 Mpx back).
/// `None` when the texture is not a cache drawn at an integer scale past one.
fn cut_scaled(d: &DrawSpec, (dx, dy, w, h): (u32, u32, u32, u32)) -> Option<DrawSpec> {
    let Tex::Cache { x, row, w: tw, h: th } = d.tex else { return None };
    let (qw, qh) = (d.quad.w as u32, d.quad.h as u32);
    if tw == 0 || th == 0 || qw % tw != 0 || qh % th != 0 || (qw == tw && qh == th) {
        return None;
    }
    let (kx, ky) = (qw / tw, qh / th);
    let span = |p0: u32, len: u32, k: u32, whole: u32| {
        let lo = (p0 / k).saturating_sub(1);
        let hi = ((p0 + len).div_ceil(k) + 1).min(whole);
        (lo, hi - lo)
    };
    let (t0x, tnw) = span(dx, w, kx, tw);
    let (t0y, tnh) = span(dy, h, ky, th);
    let part = (
        d.quad.x + (t0x * kx) as f32,
        d.quad.y + (t0y * ky) as f32,
        (tnw * kx) as f32,
        (tnh * ky) as f32,
    );
    let tex = Tex::Cache { x: x + t0x, row: row + t0y, w: tnw, h: tnh };
    Some(DrawSpec { pipe: d.pipe, quad: d.quad.part_of(part), tex })
}

/// `d` cut to `area`: `None` when its quad lies outside; itself when inside, when it is a blur
/// (its chain cut it), when its band is spread over the layer or its quad is off the pixel
/// grid; else the draw of the part they share — the quad the part, the push constants the whole
/// shape, the texture the part's texels.
pub fn clip_draw(d: &DrawSpec, area: PassArea) -> Option<DrawSpec> {
    let part = meet(&d.quad, area)?;
    let q = &d.quad;
    if part == (q.x, q.y, q.w, q.h) {
        return Some(*d);
    }
    match d.pipe {
        Pipe::Blur(_) => Some(*d),
        Pipe::Shadow => Some(DrawSpec { quad: q.part_of(part), ..*d }),
        Pipe::Opaque | Pipe::Straight | Pipe::Rounded | Pipe::Premultiplied => {
            if !on_the_grid(q) {
                return Some(*d);
            }
            let shift =
                ((part.0 - q.x) as u32, (part.1 - q.y) as u32, part.2 as u32, part.3 as u32);
            if let Some(cut) = cut_scaled(d, shift) {
                return Some(cut);
            }
            match cut_tex(d.tex, q, shift) {
                Some(tex) => Some(DrawSpec { pipe: d.pipe, quad: q.part_of(part), tex }),
                None => Some(*d),
            }
        }
    }
}
