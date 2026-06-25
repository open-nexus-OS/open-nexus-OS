// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shadow-cache key/scale helpers. The per-row CPU shadow pass
//! (`compute_shadow_row` + `draw_soft_panel_shadow_row` +
//! `composite_shadow_layer_row`) was retired in RFC-0067 P5-Final G4 — the proof
//! panel is now a GPU layer whose soft shadow is a layer effect (see
//! `runtime/scene.rs` "1·proof"), so the only shadows left are GPU-composited.
//! These cache helpers remain for the shadow-cache unit tests.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (shadow_cache_key)

use super::SHADOW_CACHE_MAX_DOWNSCALE;
use nexus_layout_types::Rgba8;

pub(crate) fn shadow_cache_key(
    box_id_hash: u64,
    width: u32,
    height: u32,
    blur_radius: u32,
    spread: i32,
    color: Rgba8,
) -> u64 {
    let mut key = box_id_hash;
    key ^= (width as u64).rotate_left(7);
    key ^= (height as u64).rotate_left(17);
    key ^= (blur_radius as u64).rotate_left(29);
    key ^= (spread as u32 as u64).rotate_left(37);
    key ^= (color.r as u64).rotate_left(3);
    key ^= (color.g as u64).rotate_left(11);
    key ^= (color.b as u64).rotate_left(19);
    key ^ (color.a as u64).rotate_left(47)
}

pub(crate) fn shadow_cache_scale(width: u32, height: u32, budget_bytes: usize) -> Option<u8> {
    if width == 0 || height == 0 {
        return None;
    }
    for scale in 1..=SHADOW_CACHE_MAX_DOWNSCALE {
        let s = scale as u32;
        let cw = width.div_ceil(s).max(1);
        let ch = height.div_ceil(s).max(1);
        if cw as usize * ch as usize * 4 <= budget_bytes {
            return Some(scale);
        }
    }
    None
}
