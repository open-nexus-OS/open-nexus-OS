// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Regressions for the two icon-corruption root causes found on TASK-0070
//! Phase 5 (both shipped in Lucide UI icons):
//!
//! 1. Stroke-piece winding cancellation: segment quads, joins and caps share
//!    one shape under the nonzero rule, but round-join/cap discs were wound
//!    OPPOSITE to the segment quads — their overlap cancelled the winding and
//!    punched a hole at every joint. A stroked circle rendered as a DOTTED
//!    ring (the `search` icon).
//! 2. Number lexing ate a second decimal point: compact path data like
//!    `1.099.092` (1.099 then .092) parsed as one invalid token, corrupting
//!    every following parameter — `message-circle`'s big bubble arc vanished.

use nexus_svg::render_svg_tinted_at;

/// The `search` icon's ring: a stroked circle with round joins. Every angle of
/// the ring midline must be opaque — holes at segment joints are the
/// winding-cancellation bug.
#[test]
fn stroked_circle_ring_has_no_joint_holes() {
    let svg = r##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" xmlns="http://www.w3.org/2000/svg"><circle cx="11" cy="11" r="8"/></svg>"##;
    let out = render_svg_tinted_at(svg, (255, 255, 255), 96, 96).expect("render");
    let (w, scale) = (out.width as usize, 4.0f32);
    let (cx, cy, r) = (11.0 * scale, 11.0 * scale, 8.0 * scale);
    for deg in 0..360 {
        let a = (deg as f32).to_radians();
        let x = (cx + r * a.cos()).round() as usize;
        let y = (cy + r * a.sin()).round() as usize;
        let alpha = out.buffer[(y * w + x) * 4 + 3];
        assert!(
            alpha > 200,
            "ring midline must be solid at {deg}° (alpha {alpha}) — joint hole (winding cancellation)"
        );
    }
}

/// The `message-circle` path: its final `a` command carries TWO implicit
/// parameter sets AND compact `1.099.092` / `0-4.777` number runs. The big
/// bubble arc must render — probe the stroke at the bubble's top, right and
/// left extremes (all far from the small lower-left tail that survived the
/// old lexer bug).
#[test]
fn compact_numbers_and_implicit_arc_repeats_render_the_bubble() {
    let svg = r##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" xmlns="http://www.w3.org/2000/svg"><path d="M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092 10 10 0 1 0-4.777-4.719"/></svg>"##;
    let out = render_svg_tinted_at(svg, (255, 255, 255), 96, 96).expect("render");
    let w = out.width as usize;
    let alpha_near = |ux: f32, uy: f32| {
        // Max alpha in a 5px box around the user-space point (4× scale).
        let (px, py) = ((ux * 4.0) as isize, (uy * 4.0) as isize);
        let mut best = 0u8;
        for dy in -2..=2isize {
            for dx in -2..=2isize {
                let (x, y) = ((px + dx).max(0) as usize, (py + dy).max(0) as usize);
                if x < w && y < w {
                    best = best.max(out.buffer[(y * w + x) * 4 + 3]);
                }
            }
        }
        best
    };
    // Bubble centre ≈ (12.3, 11.6), radius 10: top / right / left stroke points.
    for (ux, uy, where_) in [(12.3, 1.6, "top"), (22.3, 11.6, "right"), (2.3, 11.6, "left")] {
        let a = alpha_near(ux, uy);
        assert!(a > 200, "bubble stroke missing at {where_} ({ux},{uy}): alpha {a} — arc dropped");
    }
}
