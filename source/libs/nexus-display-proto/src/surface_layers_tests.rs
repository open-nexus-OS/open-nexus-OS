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
    let mut counted = buf;
    counted[11] = 1;
    assert_eq!(decode_surface_tree(&counted[..len], &mut out), None, "a motion layer missing");
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

/// The store reaching past the two bands (1340 columns): room for a motion layer's pixels.
const WIDE_STORE: u16 = 1440;

/// A settings-like window whose sheet (rest 20,30 100×200) the compositor moves: its pixels at
/// store column 1340 beside the bands, sliding up 16 px from transparent.
fn with_sheet() -> SurfaceTree {
    let mut t = settings_like();
    t.motions[0] = MotionLayerDesc {
        x: 20,
        y: 30,
        w: 100,
        h: 200,
        sx: 1340,
        sy: 869,
        id: 7,
        token: 4,
        duration_ms: 400,
        from_dx: 0,
        from_dy: 16,
        from_opacity: 0,
        regions: 0,
        scrolls: 0,
        front: false,
        backdrop: 0,
        backdrop_opacity: 0,
        backdrop_ms: 0,
        from_scale: 0,
    };
    t.motion_count = 1;
    t
}

/// TASK-0354 P2: a motion layer rides the structure — its rest, its pixels in the store, its
/// motion and what it claims round-trip, and the structure stays valid.
#[test]
fn a_motion_layer_round_trips_with_its_structure() {
    let mut t = with_sheet();
    t.regions[2] = LayerDesc { x: 30, y: 40, w: 80, h: 60, ..LayerDesc::default() };
    t.region_count = 3;
    t.motions[0].regions = 0b100;
    // In front, over a backdrop of patch 0 at 40 % fading in over 280 ms, growing in from 0.94:
    // a modal's sheet over its scrim.
    t.motions[0].front = true;
    t.motions[0].backdrop = 1;
    t.motions[0].backdrop_opacity = 102;
    t.motions[0].backdrop_ms = 280;
    t.motions[0].from_scale = 9_400;
    let mut buf = [0u8; SURFACE_TREE_MAX_LEN];
    let len = encode_surface_tree(9, &t, &mut buf);
    let mut out = SurfaceTree::EMPTY;
    assert_eq!(decode_surface_tree(&buf[..len], &mut out), Some(9));
    assert_eq!(out, t);
    assert_eq!(validate_tree(&out, 1440, 853, 2269, WIDE_STORE), Ok(()));
    assert_eq!(out.motions()[0].store_of(30, 40), (1350, 879), "a claimed region's pixels");
    assert!(out.motions()[0].claims_region(2) && !out.motions()[0].claims_region(1));
    assert!(out.motions()[0].front);
    assert_eq!(out.motions()[0].backdrop(), Some((0, 102)));
    assert_eq!(out.motions()[0].start_scale(), 9_400);
    assert_eq!(MotionLayerDesc::EMPTY.backdrop(), None, "no backdrop");
    assert_eq!(MotionLayerDesc::EMPTY.start_scale(), SCALE_ONE, "0: unscaled");
    // The longest structure still fits windowd's receive buffer.
    const _: () = assert!(SURFACE_TREE_MAX_LEN <= 512);
}

#[test]
fn test_reject_a_motion_layer_with_a_reserved_byte_or_past_the_bound() {
    let t = with_sheet();
    let mut buf = [0u8; SURFACE_TREE_MAX_LEN];
    let len = encode_surface_tree(9, &t, &mut buf);
    let mut out = SurfaceTree::EMPTY;
    let mut front = buf;
    front[len - 7] = MOTION_FRONT; // the layer's flags byte: its one defined bit
    assert!(decode_surface_tree(&front[..len], &mut out).is_some() && out.motions()[0].front);
    let mut reserved = buf;
    reserved[len - 7] = 0x02; // a reserved flag bit
    assert_eq!(decode_surface_tree(&reserved[..len], &mut out), None, "reserved bit");
    let mut over = buf;
    over[11] = (MAX_MOTION_LAYERS + 1) as u8;
    assert_eq!(decode_surface_tree(&over[..len], &mut out), None, "past the bound");
}

/// Every bound of a motion: it rests in the base, keeps its pixels in the store, moves no longer
/// and no farther than the bounds allow.
#[test]
fn test_reject_a_motion_target_outside_the_layer_s_range() {
    let check = |t: &SurfaceTree| validate_tree(t, 1440, 853, 2269, WIDE_STORE);
    let mut t = with_sheet();
    t.motions[0].y = 700; // 700 + 200 > 853
    assert_eq!(check(&t), Err(TreeError::MotionOutside));
    let mut t = with_sheet();
    t.motions[0].w = 0;
    assert_eq!(check(&t), Err(TreeError::MotionOutside), "no area");
    let mut t = with_sheet();
    t.motions[0].sy = 800; // over the base rows
    assert_eq!(check(&t), Err(TreeError::MotionStoreOutside));
    let mut t = with_sheet();
    t.motions[0].sx = 1380; // 1380 + 100 > the declared store
    assert_eq!(check(&t), Err(TreeError::MotionStoreOutside));
    let mut t = with_sheet();
    t.motions[0].duration_ms = MAX_MOTION_MS + 1;
    assert_eq!(check(&t), Err(TreeError::MotionTooLong));
    let mut t = with_sheet();
    t.motions[0].from_dy = 900; // farther than the surface is high
    assert_eq!(check(&t), Err(TreeError::MotionTooFar));
    let mut t = with_sheet();
    t.motions[0].from_dx = -1441;
    assert_eq!(check(&t), Err(TreeError::MotionTooFar));
    let mut t = with_sheet();
    t.motions[0].from_scale = SCALE_MIN - 1; // smaller than half its size
    assert_eq!(check(&t), Err(TreeError::MotionTooFar));
    t.motions[0].from_scale = SCALE_MAX + 1;
    assert_eq!(check(&t), Err(TreeError::MotionTooFar), "larger than twice its size");
    t.motions[0].from_scale = 9_600;
    assert_eq!(check(&t), Ok(()));
    t.motions[0].backdrop_ms = MAX_MOTION_MS + 1;
    assert_eq!(check(&t), Err(TreeError::MotionTooLong), "a backdrop fading past the bound");
}

/// A motion layer claims only what lies inside it, what the structure declares, and what no
/// other motion layer claims; its pixels share none with a band's or another layer's in the
/// store — while two layers may overlap at rest (a modal over a panel: drawn in their order).
#[test]
fn test_reject_a_claim_outside_a_motion_layer_or_claimed_twice() {
    let check = |t: &SurfaceTree| validate_tree(t, 1440, 853, 2269, WIDE_STORE);
    let mut t = with_sheet();
    t.motions[0].regions = 0b10; // the content pane: not inside the sheet
    assert_eq!(check(&t), Err(TreeError::ClaimOutside));
    let mut t = with_sheet();
    t.motions[0].regions = 0b1000; // region 3: not declared
    assert_eq!(check(&t), Err(TreeError::ClaimOutside));
    let mut t = with_sheet();
    t.motions[0].scrolls = 0b1; // the content viewport: not inside the sheet
    assert_eq!(check(&t), Err(TreeError::ClaimOutside));
    let mut t = with_sheet();
    t.regions[2] = LayerDesc { x: 30, y: 40, w: 10, h: 10, ..LayerDesc::default() };
    t.region_count = 3;
    t.motions[0].regions = 0b100;
    t.motions[1] = MotionLayerDesc { x: 25, sx: 1340, sy: 1100, id: 8, ..t.motions[0] };
    t.motion_count = 2;
    assert_eq!(check(&t), Err(TreeError::ClaimTwice));
    t.motions[1].regions = 0;
    assert_eq!(check(&t), Ok(()), "two layers overlapping at rest: drawn in their order");
    t.motions[1].x = 200;
    assert_eq!(check(&t), Ok(()), "beside each other");
    t.motions[1].sy = 1000; // its pixels 1000..1200 over the first's 869..1069
    assert_eq!(check(&t), Err(TreeError::Overlap), "pixels sharing the store");
    let mut t = with_sheet();
    t.motions[0].sx = 1000; // inside the content band's columns
    assert_eq!(check(&t), Err(TreeError::Overlap));
}

/// A layer's backdrop names a patch the structure declares, never one it does not.
#[test]
fn test_reject_a_backdrop_naming_a_missing_patch() {
    let check = |t: &SurfaceTree| validate_tree(t, 1440, 853, 2269, WIDE_STORE);
    let mut t = with_sheet();
    t.motions[0].backdrop = 2; // patch 1: the structure declares one patch
    t.motions[0].backdrop_opacity = 102;
    assert_eq!(check(&t), Err(TreeError::BackdropMissing));
    t.motions[0].backdrop = 1;
    assert_eq!(check(&t), Ok(()));
    t.patch_count = 0;
    assert_eq!(check(&t), Err(TreeError::BackdropMissing), "no patch at all");
}

/// A wash may ride a motion layer (`MOTION_PARENT | m`) the structure declares, never one it
/// does not.
#[test]
fn a_wash_may_ride_a_declared_motion_layer() {
    let t = with_sheet();
    let mut m = SurfaceMotion::default();
    m.washes[0] =
        Wash { parent: MOTION_PARENT, patch: 0, radius: 6, opacity: 38, x: 4, y: 10, w: 92, h: 30 };
    m.wash_count = 1;
    assert_eq!(validate_motion(&m, &t), Ok(()));
    assert_eq!(m.washes[0].motion_parent(), Some(0));
    m.washes[0].parent = MOTION_PARENT | 1;
    assert_eq!(validate_motion(&m, &t), Err(MotionError::MissingParent));
    assert_eq!(Wash { parent: 2, ..m.washes[0] }.motion_parent(), None, "a scroll parent");
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
