// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for the windowd compositor: TileMap, cursor damage,
//! damage premerge, and LUT scaling. (The CPU glass/shadow/path/layer-cache
//! tests left with their modules — cleanup-map DELETE; glass renders via the
//! nexus-gfx/gpud GPU path now.)
//! OWNERS: @ui
//! STATUS: Functional
//! TEST_COVERAGE: 7 unit tests

use super::{build_scale_lut, cursor_damage_rect, TileMap};
use crate::compositor::damage::DamageRect;
use crate::compositor::source::build_cover_luts;

#[test]
fn scale_lut_is_monotonic_and_clamped() {
    let lut = build_scale_lut(8, 3).expect("lut");
    assert_eq!(lut, vec![0, 0, 0, 1, 1, 1, 2, 2]);
    assert!(lut.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(*lut.last().unwrap_or(&u32::MAX), 2);
}

#[test]
fn cover_luts_native_mode_matches_plain_scale() {
    let (x, y) = build_cover_luts(1280, 800, 1280, 800).expect("luts");
    assert_eq!(x, build_scale_lut(1280, 1280).expect("x"));
    assert_eq!(y, build_scale_lut(800, 800).expect("y"));
}

#[test]
fn cover_luts_narrow_mode_center_crops_width() {
    // 600×800 onto a 1280×800 source: height pins the scale (1:1), the
    // width shows the centered 600-column window (offset (1280-600)/2).
    let (x, y) = build_cover_luts(600, 800, 1280, 800).expect("luts");
    assert_eq!(x[0], 340);
    assert_eq!(x[599], 340 + 599);
    assert_eq!(y[0], 0);
    assert_eq!(y[799], 799);
    assert!(x.windows(2).all(|p| p[0] <= p[1]));
    assert!(x.iter().all(|&v| v < 1280));
}

#[test]
fn cover_luts_short_mode_center_crops_height() {
    // 1280×400 onto 1280×800: width pins the scale, height center-crops.
    let (x, y) = build_cover_luts(1280, 400, 1280, 800).expect("luts");
    assert_eq!(x[0], 0);
    assert_eq!(x[1279], 1279);
    assert_eq!(y[0], 200);
    assert_eq!(y[399], 200 + 399);
    assert!(y.iter().all(|&v| v < 800));
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
