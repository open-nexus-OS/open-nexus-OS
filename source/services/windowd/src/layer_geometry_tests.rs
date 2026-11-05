// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: unit tests of `layer_geometry` — a scroll layer's pieces, a wash's place, what a new
//! structure changes on the screen (moved regions, regions and motion layers that come or go),
//! who may declare layers, the store's damage, the demand on the frame's bound. Split out of
//! `layer_geometry.rs` (module-size ratchet).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_SCOPE: the surface layers' geometry (RFC-0109, TASK-0351 P15, TASK-0354 P2)

use super::*;
use nexus_display_proto::surface_layers::{Patch, AXIS_HORIZONTAL};

fn vertical() -> ScrollLayerDesc {
    ScrollLayerDesc {
        vx: 310,
        vy: 120,
        vw: 1080,
        vh: 700,
        axis: AXIS_VERTICAL,
        flags: 0,
        bx: 0,
        by: 869,
        strip_len: 1400,
        strips: 1,
        valid_start: 0,
        valid_len: 1400,
    }
}

#[test]
fn the_visible_window_is_one_piece_inside_the_ring() {
    let mut out = [Piece::default(); MAX_PIECES];
    let n = pieces(&vertical(), 300, &mut out);
    assert_eq!(n, 1);
    assert_eq!(out[0], Piece { src: (0, 869 + 300), dst: (310, 120), size: (1080, 700) });
}

#[test]
fn the_window_across_the_ring_wrap_is_two_pieces() {
    let mut s = vertical();
    s.valid_start = 1000;
    let mut out = [Piece::default(); MAX_PIECES];
    // Content 1100..1800 sits at ring rows 1100..1400 and 0..400.
    let n = pieces(&s, 1100, &mut out);
    assert_eq!(n, 2);
    assert_eq!(out[0], Piece { src: (0, 869 + 1100), dst: (310, 120), size: (1080, 300) });
    assert_eq!(out[1], Piece { src: (0, 869), dst: (310, 420), size: (1080, 400) });
}

/// The band behind the viewport: only what the ring holds is drawn — never an evicted range.
#[test]
fn test_reject_showing_content_outside_the_valid_range() {
    let mut s = vertical();
    s.valid_start = 0;
    s.valid_len = 900;
    let mut out = [Piece::default(); MAX_PIECES];
    let n = pieces(&s, 500, &mut out);
    assert_eq!(n, 1);
    assert_eq!(out[0].size, (1080, 400), "content 500..900 only");
    s.valid_len = 0; // covered
    assert_eq!(pieces(&s, 500, &mut out), 0);
    s.valid_start = 5000;
    s.valid_len = 1400; // the ring moved on: nothing of 500..1200 is there
    assert_eq!(pieces(&s, 500, &mut out), 0);
}

#[test]
fn a_folded_horizontal_ring_cuts_at_its_strips() {
    let s = ScrollLayerDesc {
        vx: 0,
        vy: 100,
        vw: 1920,
        vh: 600,
        axis: AXIS_HORIZONTAL,
        flags: 0,
        bx: 0,
        by: 1096,
        strip_len: 1920,
        strips: 2,
        valid_start: 0,
        valid_len: 3840,
    };
    let mut out = [Piece::default(); MAX_PIECES];
    let n = pieces(&s, 1000, &mut out);
    assert_eq!(n, 2);
    assert_eq!(out[0], Piece { src: (1000, 1096), dst: (0, 100), size: (920, 600) });
    assert_eq!(out[1], Piece { src: (0, 1696), dst: (920, 100), size: (1000, 600) });
}

#[test]
fn a_wash_follows_its_scroll_layer_and_is_clipped_to_its_viewport() {
    let mut layers = SurfaceLayers::EMPTY;
    layers.tree.scrolls[0] = vertical();
    layers.tree.scroll_count = 1;
    layers.tree.patches[0] = Patch { x: 1400, y: 853 };
    layers.tree.patch_count = 1;
    layers.motion.offsets[0] = 400;
    layers.motion.offset_count = 1;
    // A row at content y 420: on the surface at 120 + 420 - 400 = 140.
    let w = Wash { parent: 1, patch: 0, radius: 10, opacity: 38, x: 0, y: 420, w: 1080, h: 41 };
    let p = wash_place(&w, &layers, (1440, 853)).expect("visible");
    assert_eq!(p, Piece { src: (1401, 854), dst: (310, 140), size: (1080, 41) });
    // A row half above the viewport: cut at its top.
    let w = Wash { y: 390, ..w };
    let p = wash_place(&w, &layers, (1440, 853)).expect("visible");
    assert_eq!((p.dst, p.size), ((310, 120), (1080, 31)));
    // A row scrolled out: nothing.
    let w = Wash { y: 100, ..w };
    assert_eq!(wash_place(&w, &layers, (1440, 853)), None);
    // A wash on the base: base coordinates.
    let w = Wash { parent: 0, x: 20, y: 30, w: 200, h: 44, ..w };
    assert_eq!(wash_place(&w, &layers, (1440, 853)).map(|p| p.dst), Some((20, 30)));
}

/// RFC-0109: a structure takes effect with the present of the pixels it names — never before;
/// a motion is checked against the structure about to be in effect; a refusal is said once.
#[test]
fn a_structure_takes_effect_with_its_present() {
    let mut layers = SurfaceLayers::EMPTY;
    let mut tree = SurfaceTree::EMPTY;
    tree.scrolls[0] = vertical();
    tree.scroll_count = 1;
    layers.stage(tree);
    assert_eq!(layers.latest_tree(), tree);
    assert_eq!(layers.tree, SurfaceTree::EMPTY, "not before its present");
    assert_eq!(layers.activate(), TreeChange::STRUCTURE);
    assert_eq!(layers.tree, tree);
    assert_eq!(layers.activate(), TreeChange::NONE, "nothing waiting");
    assert!(layers.first_refusal());
    assert!(!layers.first_refusal(), "said once");
    layers.clear();
    assert_eq!(layers.tree, SurfaceTree::EMPTY);
    assert!(layers.first_refusal(), "a new surface may be refused again");
}

/// A band's new rows show in its scroll layer's viewport; a patch's in nothing.
#[test]
fn store_rows_damage_the_viewport_of_their_band() {
    let mut layers = SurfaceLayers::EMPTY;
    layers.tree.scrolls[0] = vertical();
    layers.tree.scroll_count = 1;
    let mut out = [(0, 0, 0, 0); STORE_DAMAGE_MAX];
    assert_eq!(store_damage(&layers, (0, 1000, 1080, 64), &mut out), 1);
    assert_eq!(out[0], (310, 120, 1080, 700));
    assert_eq!(store_damage(&layers, (1400, 853, 16, 16), &mut out), 0, "a patch");
    layers.tree.scrolls[0].valid_len = 0; // covered: nothing of the band shows
    assert_eq!(store_damage(&layers, (0, 1000, 1080, 64), &mut out), 0);
}

/// RFC-0109: a client declares layers on its own surface only.
#[test]
fn test_reject_layers_for_a_surface_the_sender_does_not_own() {
    assert!(owns(0x51, 0x51));
    assert!(!owns(0x51, 0x52), "another client's surface");
    assert!(!owns(0, 0), "an unnamed creator owns nothing");
}

#[test]
fn a_new_valid_range_changes_its_band_alone() {
    let mut now = SurfaceTree::EMPTY;
    now.scrolls[0] = vertical();
    now.scroll_count = 1;
    let mut next = now;
    next.scrolls[0].valid_start = 64;
    assert_eq!(structure_change(&now, &next), TreeChange::bands(1));
    let mut moved = now;
    moved.scrolls[0].vy = 130;
    assert_eq!(structure_change(&now, &moved), TreeChange::STRUCTURE);
    assert_eq!(structure_change(&now, &now), TreeChange::NONE);
}

/// TASK-0351 P15: a panel sliding under the bar is one region moving — its old and new
/// rectangle change (its shadow with it), not the whole desktop; a region that comes or goes
/// changes where it is or was (TASK-0354 P2: a panel opening is not a full frame).
#[test]
fn a_moved_region_changes_its_old_and_new_rectangle_alone() {
    let panel = LayerDesc {
        x: 12,
        y: 44,
        w: 330,
        h: 600,
        material: 1,
        glass_level: 1,
        ..LayerDesc::default()
    };
    let mut now = SurfaceTree::EMPTY;
    now.regions[0] = LayerDesc { x: 0, y: 0, w: 1920, h: 36, ..LayerDesc::default() };
    now.regions[1] = LayerDesc { y: 34, ..panel };
    now.region_count = 2;
    let mut next = now;
    next.regions[1] = panel;
    let mut layers = SurfaceLayers::EMPTY;
    layers.tree = now;
    layers.stage(next);
    let change = layers.activate();
    assert!(!change.whole);
    assert_eq!(change.rects(), &[(12, 34, 330, 610)], "where it was and where it is");
    // The same rectangle dressed anew (a fading tint): the region's rectangle.
    let mut dressed = next;
    dressed.regions[0].radius = 8;
    layers.stage(dressed);
    assert_eq!(layers.activate().rects(), &[(0, 0, 1920, 36)]);
    // A shadowed region that appears: its rectangle and its shadow — not the structure.
    let mut appeared = dressed;
    appeared.regions[2] = LayerDesc { shadow_alpha: 80, ..panel };
    appeared.region_count = 3;
    layers.stage(appeared);
    let change = layers.activate();
    assert!(!change.whole);
    assert_eq!(change.rects(), &[(0, 12, 330 + 12 + 32, 600 + 32 + 32)]);
    // It goes again: where it was.
    layers.stage(dressed);
    assert_eq!(layers.activate().rects(), change.rects());
}

/// TASK-0354 P2: a motion layer that comes or goes changes its rest; the same layer declared
/// again — its motion over (duration 0) — changes nothing, and keeps its start.
#[test]
fn a_motion_layer_changes_its_rest_when_it_comes_or_goes() {
    let sheet = MotionLayerDesc {
        x: 20,
        y: 30,
        w: 100,
        h: 200,
        sx: 0,
        sy: 900,
        id: 3,
        token: 4,
        duration_ms: 400,
        ..MotionLayerDesc::EMPTY
    };
    let mut with = SurfaceTree::EMPTY;
    with.motions[0] = sheet;
    with.motion_count = 1;
    let mut layers = SurfaceLayers::EMPTY;
    layers.stage(with);
    assert_eq!(layers.activate().rects(), &[(20, 30, 100, 200)]);
    layers.starts[0] = 5_000;
    let mut over = with;
    over.motions[0].duration_ms = 0;
    layers.stage(over);
    assert_eq!(layers.activate(), TreeChange::NONE);
    assert_eq!(layers.starts[0], 5_000, "the same motion: its start kept");
    layers.stage(SurfaceTree::EMPTY);
    assert_eq!(layers.activate().rects(), &[(20, 30, 100, 200)]);
    assert_eq!(layers.starts, [0; MAX_MOTION_LAYERS]);
}

/// A layer with a backdrop (a modal over its scrim) changes every pixel when it comes or goes:
/// its backdrop spans the surface; the demand counts the backdrop as a layer of its own.
#[test]
fn a_motion_layer_with_a_backdrop_changes_the_whole_surface() {
    let modal = MotionLayerDesc {
        x: 800,
        y: 460,
        w: 320,
        h: 160,
        sx: 0,
        sy: 1100,
        id: 5,
        token: 3,
        duration_ms: 280,
        front: true,
        backdrop: 1,
        backdrop_opacity: 102,
        ..MotionLayerDesc::EMPTY
    };
    let mut with = SurfaceTree::EMPTY;
    with.motions[0] = modal;
    with.motion_count = 1;
    with.patches[0] = Patch { x: 0, y: 1080 };
    with.patch_count = 1;
    let mut layers = SurfaceLayers::EMPTY;
    layers.tree.patches = with.patches;
    layers.tree.patch_count = 1;
    layers.stage(with);
    assert!(layers.activate().whole, "the scrim came: every pixel");
    assert_eq!(layers.demand(), 2, "the sheet and its backdrop");
    let mut without = with;
    without.motion_count = 0;
    layers.stage(without);
    assert!(layers.activate().whole, "the scrim went: every pixel");
}

/// A surface owes frames for its motion layer until the last frame drew it at rest.
#[test]
fn a_surface_owes_frames_until_its_motion_is_drawn_at_rest() {
    let mut layers = SurfaceLayers::EMPTY;
    layers.tree.motions[0] = MotionLayerDesc {
        x: 20,
        y: 30,
        w: 100,
        h: 200,
        sx: 0,
        sy: 900,
        id: 3,
        token: 4,
        duration_ms: 400,
        ..MotionLayerDesc::EMPTY
    };
    layers.tree.motion_count = 1;
    assert!(layers.motion_frame_owed(), "not drawn yet");
    layers.starts[0] = 1_000_000_000;
    layers.drawn_at = 1_399_000_000;
    assert!(layers.motion_frame_owed(), "the last frame was a hair short of rest");
    layers.drawn_at = 1_400_000_000;
    assert!(!layers.motion_frame_owed(), "drawn at rest");
}

/// A motion layer's new pixels in the store show at its rest; a wash riding it is placed in
/// it and clipped to it.
#[test]
fn store_rows_and_washes_of_a_motion_layer_land_at_its_rest() {
    let mut layers = SurfaceLayers::EMPTY;
    layers.tree.motions[0] = MotionLayerDesc {
        x: 20,
        y: 30,
        w: 100,
        h: 200,
        sx: 0,
        sy: 900,
        id: 3,
        ..MotionLayerDesc::EMPTY
    };
    layers.tree.motion_count = 1;
    layers.tree.patches[0] = Patch { x: 1400, y: 853 };
    layers.tree.patch_count = 1;
    let mut out = [(0, 0, 0, 0); STORE_DAMAGE_MAX];
    assert_eq!(store_damage(&layers, (0, 950, 100, 8), &mut out), 1);
    assert_eq!(out[0], (20, 30, 100, 200));
    let w = Wash {
        parent: proto::MOTION_PARENT,
        patch: 0,
        radius: 6,
        opacity: 38,
        x: 4,
        y: 190,
        w: 92,
        h: 30,
    };
    let p = wash_place(&w, &layers, (1440, 853)).expect("visible");
    assert_eq!((p.dst, p.size), ((24, 220), (92, 10)), "cut at the layer's bottom");
}

/// A surface's demand on the frame's bound: each region, each piece of a scroll layer's
/// window, each wash — and each motion layer's own pixels (TASK-0354 P2).
#[test]
fn the_demand_counts_every_layer_a_declaration_composes() {
    let mut layers = SurfaceLayers::EMPTY;
    layers.tree.scrolls[0] = vertical();
    layers.tree.scroll_count = 1;
    assert_eq!(layers.demand(), 1, "one scroll layer, its window in one piece");
    layers.tree.motion_count = 1;
    assert_eq!(layers.demand(), 2, "and a motion layer");
}
