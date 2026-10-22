// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: unit tests for the RFC-0109 surface layers wire (`surface_layers.rs`): the structure
//! and the motion round trips, the ring mapping both ends share, and a `test_reject_*` for every
//! bound a receiver checks (counts, reserved bytes, bands, patches, viewports, strips, washes).
//! Split out of `surface_layers.rs` (module-size ratchet).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_SCOPE: surface tree / surface motion codecs and validation

use crate::client_surface::{
    LayerDesc, GLASS_PANEL, MATERIAL_GLASS, MATERIAL_OPAQUE, MAX_SURFACE_LAYERS,
};
use crate::surface_layers::*;

/// The store's used width of [`settings_like`]: the content band's 1080 columns and the sidebar
/// band's 260 beside it.
const STORE_W: u16 = 1340;

/// A 1440×853 window with 2269 rows: a vertical content band of 1400 rows beside a sidebar band,
/// one patch.
fn settings_like() -> SurfaceTree {
    let mut t = SurfaceTree::EMPTY;
    t.regions[0] = LayerDesc {
        x: 0,
        y: 0,
        w: 1440,
        h: 853,
        material: MATERIAL_GLASS,
        glass_level: GLASS_PANEL,
        radius: 20,
        shadow_alpha: 0,
    };
    t.regions[1] = LayerDesc {
        x: 300,
        y: 60,
        w: 1100,
        h: 780,
        material: MATERIAL_OPAQUE,
        glass_level: 0,
        radius: 20,
        shadow_alpha: 0,
    };
    t.region_count = 2;
    t.scrolls[0] = ScrollLayerDesc {
        vx: 310,
        vy: 120,
        vw: 1080,
        vh: 700,
        axis: AXIS_VERTICAL,
        flags: BAND_OPAQUE,
        bx: 0,
        by: 869,
        strip_len: 1400,
        strips: 1,
        valid_start: 0,
        valid_len: 1050,
    };
    t.scrolls[1] = ScrollLayerDesc {
        vx: 10,
        vy: 60,
        vw: 260,
        vh: 760,
        axis: AXIS_VERTICAL,
        flags: 0,
        bx: 1080,
        by: 869,
        strip_len: 1400,
        strips: 1,
        valid_start: 0,
        valid_len: 900,
    };
    t.scroll_count = 2;
    t.patches[0] = Patch { x: 0, y: 853 };
    t.patch_count = 1;
    t
}

#[test]
fn tree_round_trip() {
    let t = settings_like();
    let mut buf = [0u8; SURFACE_TREE_MAX_LEN];
    let len = encode_surface_tree(9, &t, &mut buf);
    let mut out = SurfaceTree::EMPTY;
    assert_eq!(decode_surface_tree(&buf[..len], &mut out), Some(9));
    assert_eq!(out, t);
    assert_eq!(validate_tree(&out, 1440, 853, 2269, STORE_W), Ok(()));
    // The empty structure is valid: a surface with nothing but its base.
    let empty = encode_surface_tree(9, &SurfaceTree::EMPTY, &mut buf);
    assert_eq!(decode_surface_tree(&buf[..empty], &mut out), Some(9));
    assert_eq!(out, SurfaceTree::EMPTY);
}

#[test]
fn test_reject_a_tree_longer_than_its_counts() {
    let t = settings_like();
    let mut buf = [0u8; SURFACE_TREE_MAX_LEN];
    let len = encode_surface_tree(9, &t, &mut buf);
    let mut out = SurfaceTree::EMPTY;
    assert_eq!(decode_surface_tree(&buf[..len - 1], &mut out), None, "truncated");
    let mut longer = [0u8; SURFACE_TREE_MAX_LEN + 1];
    longer[..len].copy_from_slice(&buf[..len]);
    assert_eq!(decode_surface_tree(&longer[..len + 1], &mut out), None, "trailing byte");
    let mut reserved = buf;
    reserved[11] = 1;
    assert_eq!(decode_surface_tree(&reserved[..len], &mut out), None, "reserved byte");
    let mut wrong = buf;
    wrong[3] = OP_SURFACE_MOTION;
    assert_eq!(decode_surface_tree(&wrong[..len], &mut out), None, "another op");
}

#[test]
fn test_reject_more_scroll_layers_than_the_bound() {
    let mut buf = [0u8; SURFACE_TREE_MAX_LEN];
    let len = encode_surface_tree(9, &SurfaceTree::EMPTY, &mut buf);
    let mut out = SurfaceTree::EMPTY;
    let mut over = buf;
    over[9] = (MAX_SCROLL_LAYERS + 1) as u8;
    assert_eq!(decode_surface_tree(&over[..len], &mut out), None);
    let mut over = buf;
    over[10] = (MAX_PATCHES + 1) as u8;
    assert_eq!(decode_surface_tree(&over[..len], &mut out), None);
    let mut over = buf;
    over[8] = (MAX_SURFACE_LAYERS + 1) as u8;
    assert_eq!(decode_surface_tree(&over[..len], &mut out), None);
}

#[test]
fn test_reject_a_band_outside_the_surface() {
    let mut t = settings_like();
    t.scrolls[0].by = 1000; // 1000 + 1400 > 2269 rows
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::BandOutside));
    let mut t = settings_like();
    t.scrolls[1].bx = 1300; // 1300 + 260 > 1440 columns
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::BandOutside));
}

/// The store is the columns the create declared: a band past them is outside, though inside the
/// surface's width (windowd placed only the declared store).
#[test]
fn test_reject_a_band_past_the_declared_store() {
    let t = settings_like();
    assert_eq!(validate_tree(&t, 1440, 853, 2269, 1200), Err(TreeError::BandOutside));
    let mut t = settings_like();
    t.scroll_count = 1; // the content band alone: 1080 columns
    t.patches[0] = Patch { x: 1100, y: 853 };
    assert_eq!(validate_tree(&t, 1440, 853, 2269, 1100), Err(TreeError::PatchOutside));
}

#[test]
fn test_reject_a_band_over_the_base() {
    let mut t = settings_like();
    t.scrolls[0].by = 852;
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::BandOverBase));
}

#[test]
fn test_reject_overlapping_bands() {
    let mut t = settings_like();
    t.scrolls[1].bx = 1000; // shares columns 1000..1080 with the content band
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::Overlap));
    let mut t = settings_like();
    t.patches[0] = Patch { x: 10, y: 900 }; // inside the content band
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::Overlap));
}

#[test]
fn test_reject_a_patch_outside_the_store() {
    let mut t = settings_like();
    t.patches[0] = Patch { x: 0, y: 840 }; // over the base
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::PatchOutside));
    let mut t = settings_like();
    t.patches[0] = Patch { x: 1430, y: 853 }; // past the width
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::PatchOutside));
}

#[test]
fn test_reject_a_viewport_or_region_outside_the_base() {
    let mut t = settings_like();
    t.scrolls[0].vh = 800; // 120 + 800 > 853
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::ViewportOutside));
    let mut t = settings_like();
    t.regions[1].h = 900;
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::RegionOutside));
    let mut t = settings_like();
    t.scrolls[1].vw = 0;
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::ViewportOutside));
}

#[test]
fn test_reject_short_strips_and_a_valid_range_past_the_ring() {
    let mut t = settings_like();
    t.scrolls[0].strip_len = 349; // under half the 700-row viewport
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::StripTooShort));
    let mut t = settings_like();
    t.scrolls[0].strips = 0;
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::StripTooShort));
    let mut t = settings_like();
    t.scrolls[0].valid_len = 1401;
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::ValidPastCapacity));
    let mut t = settings_like();
    t.scrolls[0].axis = 2;
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::UnknownAxisOrFlag));
    let mut t = settings_like();
    t.scrolls[0].flags = 2;
    assert_eq!(validate_tree(&t, 1440, 853, 2269, STORE_W), Err(TreeError::UnknownAxisOrFlag));
}

#[test]
fn the_ring_wraps_and_folds_into_strips() {
    let t = settings_like();
    let s = t.scrolls[0];
    assert_eq!(s.ring_place(0), Some((0, 869, 1400)));
    assert_eq!(s.ring_place(1399), Some((0, 869 + 1399, 1)));
    assert_eq!(s.ring_place(1400), Some((0, 869, 1400)), "wraps");
    // A horizontal ring of two 960-column strips stacked under each other.
    let h = ScrollLayerDesc {
        vx: 0,
        vy: 100,
        vw: 1920,
        vh: 600,
        axis: AXIS_HORIZONTAL,
        flags: 0,
        bx: 0,
        by: 1096,
        strip_len: 960,
        strips: 2,
        valid_start: 0,
        valid_len: 1920,
    };
    assert_eq!(h.capacity(), 1920);
    assert_eq!(h.band_rect(), (0, 1096, 960, 1200));
    assert_eq!(h.ring_place(10), Some((10, 1096, 950)));
    assert_eq!(h.ring_place(970), Some((10, 1696, 950)), "the second strip");
    assert_eq!(h.ring_place(1930), Some((10, 1096, 950)), "wraps to the first");
}

#[test]
fn motion_round_trip_and_validation() {
    let t = settings_like();
    let mut m = SurfaceMotion::default();
    m.offsets[0] = 360;
    m.offsets[1] = -4;
    m.offset_count = 2;
    m.washes[0] =
        Wash { parent: 1, patch: 0, radius: 10, opacity: 38, x: 0, y: 412, w: 1080, h: 41 };
    m.wash_count = 1;
    let mut buf = [0u8; SURFACE_MOTION_MAX_LEN];
    let len = encode_surface_motion(9, &m, &mut buf);
    let mut out = SurfaceMotion::default();
    assert_eq!(decode_surface_motion(&buf[..len], &mut out), Some(9));
    assert_eq!(out, m);
    assert_eq!(validate_motion(&out, &t), Ok(()));
    assert_eq!(decode_surface_motion(&buf[..len - 1], &mut out), None, "truncated");
    let mut reserved = buf;
    reserved[10] = 1;
    assert_eq!(decode_surface_motion(&reserved[..len], &mut out), None, "reserved bytes");
    let mut over = buf;
    over[9] = (MAX_WASHES + 1) as u8;
    assert_eq!(decode_surface_motion(&over[..len], &mut out), None, "over the bound");
}

#[test]
fn test_reject_a_wash_naming_a_missing_patch() {
    let t = settings_like();
    let mut m = SurfaceMotion::default();
    m.washes[0] = Wash { parent: 0, patch: 1, radius: 0, opacity: 38, x: 0, y: 0, w: 10, h: 10 };
    m.wash_count = 1;
    assert_eq!(validate_motion(&m, &t), Err(MotionError::MissingPatch));
    m.washes[0].patch = 0;
    m.washes[0].parent = 3;
    assert_eq!(validate_motion(&m, &t), Err(MotionError::MissingParent));
    m.washes[0].parent = 0;
    m.washes[0].h = 0;
    assert_eq!(validate_motion(&m, &t), Err(MotionError::EmptyWash));
}

#[test]
fn the_new_ops_are_distinct_from_every_surface_op() {
    use crate::client_surface::*;
    for op in [OP_SURFACE_TREE, OP_SURFACE_MOTION] {
        assert!(op > 4, "op {op} collides with the input family");
        for other in [
            OP_SURFACE_CREATE,
            OP_SURFACE_PRESENT,
            OP_SURFACE_DESTROY,
            OP_SURFACE_INPUT,
            OP_SURFACE_EVENTS,
            OP_SURFACE_INTENT,
            OP_SURFACE_RECT,
            OP_SURFACE_LAYERS,
            OP_SURFACE_CONTROL,
            OP_SURFACE_FRAME_REQ,
            OP_SURFACE_FRAME,
            crate::surface_capture::OP_SURFACE_CAPTURE,
            crate::surface_capture::OP_SURFACE_CAPTURE_KEY,
            crate::surface_windows::OP_SURFACE_WINDOWS,
            crate::surface_windows::OP_SURFACE_TASKBAR,
            crate::surface_settings::OP_SURFACE_SETTINGS,
        ] {
            assert_ne!(op, other);
        }
    }
}
