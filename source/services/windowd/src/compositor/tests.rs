// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for the windowd compositor: TileMap, backdrop cache,
//! path cache, shadow cache, cursor damage, layer cache, and LUT scaling.
//! OWNERS: @ui
//! STATUS: Functional
//! TEST_COVERAGE: 13 unit tests

use super::{
    build_scale_lut, cursor_damage_rect, layer_cache_key, path_cache_slot, path_id_hash,
    record_layer_cache_row, shadow_cache_key, shadow_cache_scale, LayerCache, ProofBoxRect,
    TileMap, TILES_X, TILES_Y, TILE_SIZE,
};
use crate::live_runtime::{DamageRect, GlassQuality};
use nexus_layout_types::Rgba8;

#[test]
fn scale_lut_is_monotonic_and_clamped() {
    let lut = build_scale_lut(8, 3).expect("lut");
    assert_eq!(lut, vec![0, 0, 0, 1, 1, 1, 2, 2]);
    assert!(lut.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(*lut.last().unwrap_or(&u32::MAX), 2);
}

#[test]
fn path_cache_slot_is_stable_for_same_key() {
    let id_hash = path_id_hash("card_hover_glyph");
    let a = path_cache_slot(id_hash, 16, 16, [1, 2, 3, 255], 8);
    let b = path_cache_slot(id_hash, 16, 16, [1, 2, 3, 255], 8);
    let c = path_cache_slot(id_hash, 24, 16, [1, 2, 3, 255], 8);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn shadow_cache_scale_keeps_large_panel_inside_fixed_budget() {
    let scale = shadow_cache_scale(920, 360, 16 * 1024).expect("scaled cache");
    let cache_w = 920u32.div_ceil(u32::from(scale));
    let cache_h = 360u32.div_ceil(u32::from(scale));
    assert!(cache_w as usize * cache_h as usize * 4 <= 16 * 1024);
    assert!(scale > 1);
}

#[test]
fn damage_premerge_merges_only_bounded_area_growth() {
    let mut rects = [
        DamageRect { x: 10, y: 10, width: 20, height: 20 },
        DamageRect { x: 25, y: 10, width: 20, height: 20 },
        DamageRect { x: 400, y: 400, width: 20, height: 20 },
    ];
    let count = super::premerge_damage_rects(&mut rects, 3);
    assert_eq!(count, 2);
    assert!(rects[..count].iter().any(|rect| rect.width == 35 && rect.height == 20));
    assert!(rects[..count].iter().any(|rect| rect.x == 400 && rect.y == 400));
}

#[test]
fn tile_map_has_dirty_in_row_range_detects_marked_rows() {
    let mut tm = TileMap::new();
    // Mark a rect covering rows 128..192 (tiles ty=2..=2)
    tm.mark_rect(DamageRect { x: 0, y: 128, width: 1280, height: 64 });
    assert!(tm.has_dirty_in_row_range(128, 192));
    // Row range outside the marked area should be clean
    assert!(!tm.has_dirty_in_row_range(0, 64));
    assert!(!tm.has_dirty_in_row_range(256, 320));
}

#[test]
fn tile_map_has_dirty_in_row_range_partial_overlap() {
    let mut tm = TileMap::new();
    // Mark tile rows 2..=3 (y=128..256)
    tm.mark_rect(DamageRect { x: 0, y: 140, width: 1280, height: 100 });
    // Row range that only partially overlaps should still be dirty
    assert!(tm.has_dirty_in_row_range(120, 180));
    assert!(tm.has_dirty_in_row_range(200, 300));
}

#[test]
fn tile_map_clear_resets_all_dirty() {
    let mut tm = TileMap::new();
    tm.mark_rect(DamageRect { x: 0, y: 0, width: 1280, height: 800 });
    assert!(tm.has_dirty());
    tm.clear();
    assert!(!tm.has_dirty());
    assert!(!tm.has_dirty_in_row_range(0, 800));
}

#[test]
fn cursor_damage_rect_clips_hotspot_and_edges() {
    let rect = cursor_damage_rect(1, 1, 32, 32, 1280, 800).expect("visible cursor");
    assert_eq!(rect, DamageRect { x: 0, y: 0, width: 31, height: 31 });

    let offscreen = cursor_damage_rect(-80, -80, 32, 32, 1280, 800);
    assert!(offscreen.is_none());
}

#[test]
fn cursor_damage_merge_covers_old_and_new_bounds_once() {
    let old_rect = cursor_damage_rect(100, 100, 32, 32, 1280, 800).expect("old cursor");
    let new_rect = cursor_damage_rect(116, 112, 32, 32, 1280, 800).expect("new cursor");
    assert_eq!(old_rect.merge(new_rect), DamageRect { x: 98, y: 98, width: 48, height: 44 });
}

#[test]
fn shadow_cache_key_includes_shape_and_effect_params() {
    let color = Rgba8::new(1, 2, 3, 180);
    let base = shadow_cache_key(7, 64, 32, 6, 2, color);
    assert_ne!(base, shadow_cache_key(7, 65, 32, 6, 2, color));
    assert_ne!(base, shadow_cache_key(7, 64, 33, 6, 2, color));
    assert_ne!(base, shadow_cache_key(7, 64, 32, 7, 2, color));
    assert_ne!(base, shadow_cache_key(7, 64, 32, 6, 3, color));
    assert_ne!(base, shadow_cache_key(7, 64, 32, 6, 2, Rgba8::new(1, 2, 3, 181)));
}

#[test]
fn layer_cache_populates_rows_and_serves_clean_layer() {
    let mut cache = LayerCache::default();
    let key = layer_cache_key("proof_panel");
    let rect = ProofBoxRect { x: 4, y: 10, width: 2, height: 2 };
    let mut row0 = vec![0u8; 8 * 4];
    let mut row1 = vec![0u8; 8 * 4];
    row0[16..24].copy_from_slice(&[1, 2, 3, 255, 4, 5, 6, 255]);
    row1[16..24].copy_from_slice(&[7, 8, 9, 255, 10, 11, 12, 255]);

    record_layer_cache_row(&mut cache, key, rect, 10, &row0, 255, None).expect("row 0 cache");
    assert!(cache.get(key).expect("layer").dirty);

    record_layer_cache_row(&mut cache, key, rect, 11, &row1, 255, None).expect("row 1 cache");
    let layer = cache.get(key).expect("layer");
    assert!(!layer.dirty);
    assert_eq!(layer.pixels, [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]);
}
