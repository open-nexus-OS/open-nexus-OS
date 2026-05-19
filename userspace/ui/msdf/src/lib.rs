// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Multi-channel Signed Distance Field (MSDF) atlas for text and icon rendering.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 22 tests (tests/ui_v4_host/src/msdf_tests.rs)
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md
//!
//! Build-time: `build.rs` renders each printable ASCII glyph (32-126) as a 32×32
//! signed distance field, packs them into a single atlas texture, and emits a
//! glyph metrics table. The atlas is embedded via `include_bytes!`.
//!
//! Runtime: `sample_atlas(ch, u, v)` bilinear-samples the atlas for a character
//! and returns a coverage value via `smoothstep`. All computation is integer-only,
//! `no_std` + `alloc`, allocation-free in the hot path.

#![no_std]

extern crate alloc;

mod generated {
    #![allow(dead_code, clippy::excessive_precision)]
    include!(concat!(env!("OUT_DIR"), "/msdf_metrics.rs"));
}

pub use generated::{
    MSDF_ATLAS, MSDF_ATLAS_HEIGHT, MSDF_ATLAS_WIDTH, MSDF_FIRST_CHAR, MSDF_GLYPH_COUNT,
    MSDF_GLYPH_SIZE, MSDF_METRICS,
};

/// Per-glyph layout metrics, matching the struct emitted by `build.rs`.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct GlyphMetrics {
    pub atlas_col: u32,
    pub atlas_row: u32,
    pub advance: f32,
    pub bearing_x: f32,
    pub bearing_y: f32,
    pub width: u32,
    pub height: u32,
}

/// Look up glyph metrics for a character.
/// Returns `None` if the character is outside the atlas range.
pub fn glyph_metrics(ch: char) -> Option<&'static GlyphMetrics> {
    let code = ch as u32;
    if code < MSDF_FIRST_CHAR {
        return None;
    }
    let idx = (code - MSDF_FIRST_CHAR) as usize;
    MSDF_METRICS.get(idx)
}

/// Sample the SDF atlas at a glyph-relative UV coordinate.
///
/// `ch`: the character to sample.
/// `u`, `v`: normalized coordinates within the glyph cell (0.0–1.0).
///
/// Returns the SDF value (0-255), where 128 is the glyph edge, <128 is outside,
/// and >128 is inside. Uses bilinear interpolation for smooth scaling.
pub fn sample_atlas(ch: char, u: f32, v: f32) -> u8 {
    let metrics = match glyph_metrics(ch) {
        Some(m) => m,
        None => return 0,
    };

    let cell_x = metrics.atlas_col * MSDF_GLYPH_SIZE;
    let cell_y = metrics.atlas_row * MSDF_GLYPH_SIZE;
    let cell_w = MSDF_GLYPH_SIZE;
    let cell_h = MSDF_GLYPH_SIZE;

    // Map UV to atlas pixel coordinates (floating point)
    let px = u * (cell_w as f32 - 1.0);
    let py = v * (cell_h as f32 - 1.0);

    // Integer pixel coordinates for bilinear interpolation
    let x0 = px as u32;
    let y0 = py as u32;
    let x1 = (x0 + 1).min(cell_w - 1);
    let y1 = (y0 + 1).min(cell_h - 1);

    let fx = px - x0 as f32;
    let fy = py - y0 as f32;

    let atlas_w = MSDF_ATLAS_WIDTH;
    let atlas_stride = (atlas_w * 4) as usize;

    // Sample four corners (use alpha channel; all channels are identical for SDF)
    let s00 = atlas_sample((cell_x + x0, cell_y + y0), atlas_stride);
    let s10 = atlas_sample((cell_x + x1, cell_y + y0), atlas_stride);
    let s01 = atlas_sample((cell_x + x0, cell_y + y1), atlas_stride);
    let s11 = atlas_sample((cell_x + x1, cell_y + y1), atlas_stride);

    // Bilinear interpolation
    let s0 = s00 as f32 + (s10 as f32 - s00 as f32) * fx;
    let s1 = s01 as f32 + (s11 as f32 - s01 as f32) * fx;
    (s0 + (s1 - s0) * fy) as u8
}

/// Convert an SDF value to alpha coverage via smoothstep.
///
/// `sd`: signed distance value (0-255, 128 = edge).
/// `aa_width`: anti-aliasing width in SDF units (typical: 8-16 for 32px glyphs).
///
/// Returns alpha (0-255): 0 = fully outside, 255 = fully inside.
pub fn sdf_to_alpha(sd: u8, aa_width: u8) -> u8 {
    let half_aa = aa_width as i32;
    let edge = 128i32;
    let sd = sd as i32;
    // smoothstep(edge - half_aa, edge + half_aa, sd)
    let t = (sd - (edge - half_aa)).clamp(0, 2 * half_aa);
    ((t * 255) / (2 * half_aa)) as u8
}

/// Convenience: sample atlas and convert to alpha in one call.
pub fn sample_alpha(ch: char, u: f32, v: f32, aa_width: u8) -> u8 {
    let sd = sample_atlas(ch, u, v);
    sdf_to_alpha(sd, aa_width)
}

fn atlas_sample((x, y): (u32, u32), stride: usize) -> u8 {
    let idx = (y as usize).wrapping_mul(stride).wrapping_add(x as usize * 4);
    MSDF_ATLAS.get(idx).copied().unwrap_or(0)
}