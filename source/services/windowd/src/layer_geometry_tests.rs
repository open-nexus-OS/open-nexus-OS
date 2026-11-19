// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: unit tests of `layer_geometry` — a scroll layer's pieces, a wash's place, what a new
//! structure changes on the screen (moved regions, regions, motion and scroll layers that come
//! or go),
//! who may declare layers, the store's damage, the demand on the frame's bound. Split out of
//! `layer_geometry.rs` (module-size ratchet).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_SCOPE: the surface layers' geometry (RFC-0109, TASK-0351 P15, TASK-0354 P2)

use super::*;
use crate::motion_geometry::{drawn_moved, moved_at, Moved, Segment};
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

/// One launcher page (a band of one viewport) — the paged track's rubber band (TASK-0354 P12).
fn one_page() -> ScrollLayerDesc {
    ScrollLayerDesc {
        vx: 0,
        vy: 100,
        vw: 1280,
        vh: 600,
        axis: AXIS_HORIZONTAL,
        flags: 0,
        bx: 0,
        by: 1096,
        strip_len: 1280,
        strips: 1,
        valid_start: 0,
        valid_len: 1280,
    }
}

/// A paged track pulled 35 px past its start (offset −35): the band is shown moved in by 35 px,
/// the strip it leaves shows the base — windowd reads nothing before the band.
#[test]
fn a_band_pulled_past_its_start_is_shown_moved_in() {
    let mut out = [Piece::default(); MAX_PIECES];
    let n = pieces(&one_page(), -35, &mut out);
    assert_eq!(n, 1);
    assert_eq!(out[0], Piece { src: (0, 1096), dst: (35, 100), size: (1280 - 35, 600) });
    // …and past its end (offset +35): moved out by 35 px.
    let n = pieces(&one_page(), 35, &mut out);
    assert_eq!(n, 1);
    assert_eq!(out[0], Piece { src: (35, 1096), dst: (0, 100), size: (1280 - 35, 600) });
}

/// Any `i32` offset is intersected with the band: at the integer's ends nothing is shown and
/// nothing panics.
#[test]
fn test_reject_an_offset_at_the_i32_ends() {
    let mut out = [Piece::default(); MAX_PIECES];
    assert_eq!(pieces(&one_page(), i32::MIN, &mut out), 0);
    assert_eq!(pieces(&one_page(), i32::MAX, &mut out), 0);
    assert_eq!(pieces(&one_page(), -1280, &mut out), 0, "one viewport before: all base");
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
    let came = layers.activate();
    assert!(!came.whole, "a scroll layer that came is its viewport");
    assert_eq!(came.rects(), &[(310, 120, 1080, 700)]);
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
    let change = structure_change(&now, &moved);
    assert!(!change.whole, "a re-placed band is its viewports, not the surface");
    assert_eq!(change.rects(), &[(310, 120, 1080, 700), (310, 130, 1080, 700)]);
    assert_eq!(structure_change(&now, &now), TreeChange::NONE);
}

/// TASK-0351 P15: the launcher brings its grid's band and takes it away — the viewport changes,
/// not every pixel of the desktop; a band whose index shifted because another came keeps its
/// pixels, and a new range of it is its band's rows at its new index.
#[test]
fn a_scroll_layer_changes_its_viewport_when_it_comes_or_goes() {
    let grid = ScrollLayerDesc { vx: 160, vy: 200, vw: 1600, vh: 640, by: 1300, ..vertical() };
    let mut now = SurfaceTree::EMPTY;
    let mut next = now;
    next.scrolls[0] = grid;
    next.scroll_count = 1;
    let came = structure_change(&now, &next);
    assert!(!came.whole);
    assert_eq!(came.rects(), &[(160, 200, 1600, 640)], "where it is");
    assert_eq!(came.bands, 0);
    let went = structure_change(&next, &now);
    assert!(!went.whole);
    assert_eq!(went.rects(), &[(160, 200, 1600, 640)], "where it was");
    now.scrolls[0] = vertical();
    now.scroll_count = 1;
    let mut both = now;
    both.scrolls[0] = grid;
    both.scrolls[1] = ScrollLayerDesc { valid_start: 64, ..vertical() };
    both.scroll_count = 2;
    let shifted = structure_change(&now, &both);
    assert_eq!(shifted.rects(), &[(160, 200, 1600, 640)], "the new band alone");
    assert_eq!(shifted.bands, 1 << 1, "the shifted band's new range, at its new index");
    let mut patched = now;
    patched.patches[0] = Patch { x: 0, y: 1080 };
    patched.patch_count = 1;
    assert_eq!(structure_change(&now, &patched), TreeChange::STRUCTURE, "patches are the surface");
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
    layers.motion_states[0] = MotionState::started(5_000);
    let mut over = with;
    over.motions[0].duration_ms = 0;
    layers.stage(over);
    assert_eq!(layers.activate(), TreeChange::NONE);
    assert_eq!(layers.motion_states[0].seg.start, 5_000, "the same motion: its start kept");
    layers.stage(SurfaceTree::EMPTY);
    assert_eq!(layers.activate().rects(), &[(20, 30, 100, 200)]);
    assert_eq!(layers.motion_states, [MotionState::EMPTY; MAX_MOTION_LAYERS]);
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
        backdrop_ms: 180,
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
    let mut over = with;
    over.motions[0].duration_ms = 0;
    over.motions[0].backdrop_ms = 0;
    layers.stage(over);
    assert_eq!(layers.activate(), TreeChange::NONE, "the motion and its scrim's fade are over");
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
    layers.motion_states[0] = MotionState::started(1_000_000_000);
    layers.drawn_at = 1_399_000_000;
    assert!(layers.motion_frame_owed(), "the last frame was a hair short of rest");
    layers.drawn_at = 1_400_000_000;
    assert!(!layers.motion_frame_owed(), "drawn at rest");
}

const MS: u64 = 1_000_000;

/// A sheet sliding up 16 px over 300 ms (id 3), leaving when `exit`.
fn sliding(exit: bool) -> SurfaceTree {
    let mut tree = SurfaceTree::EMPTY;
    tree.motions[0] = MotionLayerDesc {
        x: 20,
        y: 30,
        w: 100,
        h: 200,
        sx: 0,
        sy: 900,
        id: 3,
        token: 4,
        duration_ms: 300,
        from_dy: 16,
        exit,
        ..MotionLayerDesc::EMPTY
    };
    tree.motion_count = 1;
    tree
}

/// `layers` declared `tree`, its enter drawn from `at` to its end.
fn entered(layers: &mut SurfaceLayers, tree: SurfaceTree, at: u64) {
    layers.stage(tree);
    let _ = layers.activate();
    assert_eq!(layers.stamp(at, FrameNote::default())[0], Said::Started);
    let _ = layers.stamp(at + 300 * MS, FrameNote::default());
    assert!(!layers.motion_frame_owed(), "the enter drawn at rest");
}

/// TASK-0354 P9: a close or a re-open flips the direction of the same layer — the same place,
/// pixels and claims — and changes no rectangle (windowd moves it; one over a scrim is not the
/// whole surface either); the flip owes its frame, and a flip back before that frame leaves the
/// layer as it was.
#[test]
fn a_direction_flip_changes_no_rectangle() {
    let mut layers = SurfaceLayers::EMPTY;
    entered(&mut layers, sliding(false), 1_000 * MS);
    layers.stage(sliding(true));
    assert_eq!(layers.activate(), TreeChange::NONE, "a close");
    assert!(layers.motion_frame_owed(), "the flip owes its frame");
    layers.stage(sliding(false));
    assert_eq!(layers.activate(), TreeChange::NONE, "a re-open before any frame");
    assert!(!layers.motion_frame_owed(), "the enter it interrupted never stopped: at rest");
    let mut modal = sliding(false);
    modal.motions[0].backdrop = 1;
    modal.motions[0].backdrop_ms = 180;
    let mut closing = modal;
    closing.motions[0].exit = true;
    let mut layers = SurfaceLayers::EMPTY;
    entered(&mut layers, modal, 1_000 * MS);
    layers.stage(closing);
    assert_eq!(layers.activate(), TreeChange::NONE, "a modal's close: not the whole surface");
}

/// TASK-0354 P9: a close starts where the layer rests and plays to its from-state; past its end
/// the layer keeps its transform at opacity 0 — drawn through it, never at rest — and owes no
/// frame; a motion's end is said once, with the frames drawn while it moved.
#[test]
fn an_exited_layer_is_not_drawn_and_owes_no_frame() {
    let mut layers = SurfaceLayers::EMPTY;
    entered(&mut layers, sliding(false), 1_000 * MS);
    layers.stage(sliding(true));
    let _ = layers.activate();
    let e = 3_000 * MS;
    let note = FrameNote { period_ns: 100 * MS, damage_px: 7 };
    assert_eq!(layers.stamp(e, note)[0], Said::Started, "the close starts with its frame");
    let (m, seg) = (layers.tree.motions[0], layers.motion_states[0].seg);
    assert_eq!(seg.turn, None, "the enter was over: the design curve from its rest");
    assert_eq!(moved_at(&m, &seg, e), Moved::REST, "its first frame: where it rested");
    assert!(layers.motion_frame_owed());
    assert_eq!(layers.stamp(e + 100 * MS, note)[0], Said::Nothing);
    assert_eq!(layers.stamp(e + 250 * MS, FrameNote { damage_px: 9, ..note })[0], Said::Nothing);
    let Said::Done(d) = layers.stamp(e + 300 * MS, note)[0] else { panic!("said when it ended") };
    let done = (d.frames, d.of, d.late_max_us, d.damage_px_max);
    assert_eq!(done, (3, 3, 50_000, 9), "3 frames of 3 periods, one 50 ms late, 9 px at most");
    assert!(!layers.motion_frame_owed(), "past its end: no frame owed");
    assert_eq!(layers.stamp(e + 400 * MS, note)[0], Said::Nothing, "said once");
    for later in [e + 300 * MS, e + 10_000 * MS] {
        assert!(drawn_moved(&m, &seg, later), "drawn through its transform");
        let gone = moved_at(&m, &seg, later);
        assert_eq!(gone.opacity, 0, "never drawn at rest");
        assert_eq!((gone.dx, gone.dy), (0, 16), "at its from-state");
    }
    let rest = layers.motion_states[0];
    layers.stage(sliding(true));
    let _ = layers.activate();
    assert_eq!(layers.motion_states[0], rest, "declared again: still gone, nothing owed");
    assert!(!layers.motion_frame_owed());
    let at_rest = Segment::enter_at(1_000 * MS);
    assert!(!drawn_moved(&m, &at_rest, e), "an enter at rest: the surface's own layer");
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
