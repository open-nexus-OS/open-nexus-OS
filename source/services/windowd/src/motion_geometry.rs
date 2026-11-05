// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a declared layer windowd moves (RFC-0109 §motion layers, TASK-0354 P2) — the pure
//! part: where a motion layer is drawn at the time a frame is shown (the token's curve from the
//! offset and opacity it starts at to its rest), when its motion started (the frame that first
//! drew its id; the same id declared again continues), which regions, scroll layers and washes
//! move with it, and the transform ids its layers and its backdrop carry while it moves (layers
//! may overlap: the last drawn holds a point). The app painted the layer once and is out of the
//! frame loop; windowd evaluates the motion for each frame's shown time (RFC-0110) and gpud draws
//! the layers moved — keeping a moving glass's last blur (TASK-0336 P3).
//! Host-tested here; the OS-only runtime (`compositor::runtime::surface_motion`) applies it.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

use crate::frame_transform::SCALE_ONE;
use animation::MotionToken;
use nexus_display_proto::surface_layers::{MotionLayerDesc, SurfaceTree, MAX_MOTION_LAYERS};

/// Where a motion layer is drawn: an offset from its rest (px), an opacity (`255` = drawn) and a
/// scale about its centre (1/10000, `SCALE_ONE` = its size).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Moved {
    pub(crate) dx: i32,
    pub(crate) dy: i32,
    pub(crate) opacity: u32,
    pub(crate) scale: u32,
}

impl Moved {
    /// At rest: where the app painted it, fully drawn, at its size.
    pub(crate) const REST: Self = Self { dx: 0, dy: 0, opacity: 255, scale: SCALE_ONE };
}

/// The transform ids of motion layers start above every window key (`1..=0xFFFE`, RFC-0114 §7)
/// and below `ORIGIN_ABOVE` (`u32::MAX`).
const MOTION_ID_BASE: u32 = 0x4000_0000;

/// The transform id motion layer `k` of the surface whose layers belong to `owner` (a window's
/// key, or the desktop's) carries while it moves — unique per surface and layer.
pub(crate) const fn layer_id(owner: u32, k: usize) -> u32 {
    MOTION_ID_BASE | ((owner & 0xFFFF) << 4) | (k as u32 & 0x7)
}

/// The bit of a backdrop's transform id beside its layer's (`k` takes the bits below it).
const BACKDROP_BIT: u32 = 0x8;

/// The transform id motion layer `k`'s backdrop carries while the layer moves: the layer's
/// opacity, never its offset (a scrim spans the surface — moved, it would bare an edge).
pub(crate) const fn backdrop_id(owner: u32, k: usize) -> u32 {
    layer_id(owner, k) | BACKDROP_BIT
}

/// Whether every motion layer of `tree` names a token of the curated set — a structure naming
/// another is refused whole (the curve is windowd's to resolve, ADR-0031).
pub(crate) fn tokens_known(tree: &SurfaceTree) -> bool {
    tree.motions().iter().all(|m| MotionToken::from_id(m.token).is_some())
}

/// The motion's length in ns: the layer's own, or its backdrop's fade when that is longer.
fn length_ns(m: &MotionLayerDesc) -> u64 {
    let backdrop = if m.backdrop().is_some() { m.backdrop_ms } else { 0 };
    u64::from(m.duration_ms.max(backdrop)) * 1_000_000
}

/// Where motion layer `m` is drawn at `at` (ns), its motion started at `start` (`0`: not drawn
/// yet — its first frame shows where it starts): the token's curve from the offset, opacity and
/// scale it starts at to its rest (a spring passes its rest and comes back). A motion past its
/// length, of no length, or naming an unknown token rests.
pub(crate) fn moved_at(m: &MotionLayerDesc, start: u64, at: u64) -> Moved {
    let Some(token) = MotionToken::from_id(m.token) else { return Moved::REST };
    let len = u64::from(m.duration_ms) * 1_000_000;
    if len == 0 {
        return Moved::REST;
    }
    let t = if start == 0 { 0.0 } else { at.saturating_sub(start) as f32 / len as f32 };
    if t >= 1.0 {
        return Moved::REST;
    }
    let e = token.easing().ease(t);
    let left = 1.0 - e;
    let from_op = f32::from(m.from_opacity);
    let from_scale = m.start_scale() as f32;
    let one = SCALE_ONE as f32;
    Moved {
        dx: (f32::from(m.from_dx) * left) as i32,
        dy: (f32::from(m.from_dy) * left) as i32,
        opacity: (from_op + (255.0 - from_op) * e).clamp(0.0, 255.0) as u32,
        scale: (from_scale + (one - from_scale) * e).clamp(100.0, 40_000.0) as u32,
    }
}

/// The opacity multiplier of motion layer `m`'s backdrop at `at` (its motion started at `start`):
/// the scrim fades in over its own `backdrop_ms` like a fade token — it neither moves nor springs,
/// and arrives before the sheet settles. Of no length: drawn at once.
pub(crate) fn backdrop_at(m: &MotionLayerDesc, start: u64, at: u64) -> u32 {
    let len = u64::from(m.backdrop_ms) * 1_000_000;
    if len == 0 || MotionToken::from_id(m.token).is_none() {
        return 255;
    }
    let t = if start == 0 { 0.0 } else { at.saturating_sub(start) as f32 / len as f32 };
    (255.0 * MotionToken::Fade.easing().ease(t)).clamp(0.0, 255.0) as u32
}

/// Whether motion layer `m`, started at `start`, still moves at `at` — a frame is owed for it:
/// not drawn yet, or short of its length.
pub(crate) fn in_flight(m: &MotionLayerDesc, start: u64, at: u64) -> bool {
    let len = length_ns(m);
    len > 0 && MotionToken::from_id(m.token).is_some() && (start == 0 || at < start + len)
}

/// Whether a surface whose layers the last frame drew for shown time `drawn_at` owes another
/// frame for its motion layers: one of them was not drawn yet, or was still moving in that frame
/// — a motion ends with a frame drawn at rest, never with whichever frame the clock last allowed
/// (a layer left a hair short of rest stays faded and keeps the blur of its first frame).
pub(crate) fn frame_owed(
    tree: &SurfaceTree,
    starts: &[u64; MAX_MOTION_LAYERS],
    drawn_at: u64,
) -> bool {
    tree.motions().iter().enumerate().any(|(k, m)| in_flight(m, starts[k], drawn_at))
}

/// The start times of `next`'s motion layers: a layer whose id `now` declared keeps its start —
/// the same motion continues across structures — and a new id starts with the frame that first
/// draws it (`0`).
pub(crate) fn carry_starts(
    now: &SurfaceTree,
    starts: &[u64; MAX_MOTION_LAYERS],
    next: &SurfaceTree,
) -> [u64; MAX_MOTION_LAYERS] {
    let mut out = [0; MAX_MOTION_LAYERS];
    for (slot, m) in out.iter_mut().zip(next.motions()) {
        if let Some(k) = now.motions().iter().position(|o| o.id == m.id) {
            *slot = starts[k];
        }
    }
    out
}

/// The motion layer region `k` moves with, if one claims it.
pub(crate) fn region_owner(tree: &SurfaceTree, k: usize) -> Option<usize> {
    tree.motions().iter().position(|m| m.claims_region(k))
}

/// The motion layer scroll layer `k` moves with, if one claims it.
pub(crate) fn scroll_owner(tree: &SurfaceTree, k: usize) -> Option<usize> {
    tree.motions().iter().position(|m| m.claims_scroll(k))
}

/// The topmost motion layer whose rest holds the rectangle `r` (surface coordinates) — what a
/// thing the app does not claim explicitly (the caret) moves with. Layers are drawn in their
/// order: of two overlapping, the later one holds what lies in both.
pub(crate) fn holder(tree: &SurfaceTree, r: (u32, u32, u32, u32)) -> Option<usize> {
    tree.motions().iter().rposition(|m| {
        let (x, y, w, h) = m.rest();
        r.0 >= x && r.1 >= y && r.0 + r.2 <= x + w && r.1 + r.3 <= y + h
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    /// A sheet sliding up 16 px from transparent over 400 ms (`slideUp`: the design's entrance
    /// curve, a fast arrival and a long settle).
    fn sheet() -> MotionLayerDesc {
        MotionLayerDesc {
            x: 20,
            y: 30,
            w: 100,
            h: 200,
            sx: 1340,
            sy: 869,
            id: 7,
            token: MotionToken::SlideUp.id(),
            duration_ms: 400,
            from_dy: 16,
            from_opacity: 0,
            ..MotionLayerDesc::EMPTY
        }
    }

    /// TASK-0354 P2/P3: the motion is the token's curve at the shown time — its first frame where
    /// it starts, a tenth of the way in the entrance curve already half there (0.49), its end and
    /// anything after at rest — the same values the app's keyframe track computes for the token.
    #[test]
    fn a_motion_follows_its_token_to_rest_at_the_shown_time() {
        let m = sheet();
        let start_state = Moved { dx: 0, dy: 16, opacity: 0, scale: SCALE_ONE };
        assert_eq!(moved_at(&m, 0, 5 * MS), start_state, "not drawn yet");
        let start = 1_000 * MS;
        assert_eq!(moved_at(&m, start, start), start_state);
        // A tenth of the length: the entrance curve is at 0.4944 — 8 of 16 px left, half drawn.
        let tenth = Moved { dx: 0, dy: 8, opacity: 126, scale: SCALE_ONE };
        assert_eq!(moved_at(&m, start, start + 40 * MS), tenth);
        assert_eq!(moved_at(&m, start, start + 400 * MS), Moved::REST);
        assert_eq!(moved_at(&m, start, start + 9_000 * MS), Moved::REST);
        // The app's own track for the same token and time: the same offset.
        let mut track = animation::KeyframeTrack::new(
            alloc::vec![(0.0, 16.0), (1.0, 0.0)],
            400 * MS,
            MotionToken::SlideUp.easing(),
        );
        assert_eq!(track.step(40 * MS) as i32, 8);
    }

    /// TASK-0354 P3: an entrance that grows in (a dialog from 0.94) is drawn at its starting
    /// scale first and at its size at rest; a spring passes its size before it settles.
    #[test]
    fn a_motion_grows_in_from_its_starting_scale() {
        let m = MotionLayerDesc {
            token: MotionToken::FadeScale.id(),
            duration_ms: 500,
            from_dy: 10,
            from_scale: 9_400,
            ..sheet()
        };
        let start = 1_000 * MS;
        assert_eq!(moved_at(&m, start, start).scale, 9_400);
        assert_eq!(moved_at(&m, start, start + 500 * MS), Moved::REST);
        let peak = moved_at(&m, start, start + 250 * MS);
        assert!(peak.scale > SCALE_ONE, "the spring passes its size: {}", peak.scale);
        assert!(peak.opacity == 255, "drawn fully at the spring's peak");
    }

    /// TASK-0354 P3: a backdrop fades in over its own length on the fade curve, moves never, and
    /// keeps the layer owed frames while it fades.
    #[test]
    fn a_backdrop_fades_in_over_its_own_length() {
        let m = MotionLayerDesc {
            token: MotionToken::FadeScale.id(),
            duration_ms: 200,
            backdrop: 1,
            backdrop_opacity: 102,
            backdrop_ms: 280,
            ..sheet()
        };
        let start = 1_000 * MS;
        assert_eq!(backdrop_at(&m, start, start), 0, "its first frame: transparent");
        assert_eq!(backdrop_at(&m, start, start + 140 * MS), 197, "half its length: 0.7756");
        assert_eq!(backdrop_at(&m, start, start + 280 * MS), 255);
        assert!(
            in_flight(&m, start, start + 250 * MS),
            "the layer rests, its backdrop still fades"
        );
        assert!(!in_flight(&m, start, start + 280 * MS));
        let none = MotionLayerDesc { backdrop_ms: 0, ..m };
        assert_eq!(backdrop_at(&none, start, start), 255, "no length: drawn at once");
    }

    /// A motion is owed frames until it reaches its length, never after; one of no length (the
    /// reduced-motion theme) or naming an unknown token never moves.
    #[test]
    fn a_motion_owes_frames_until_its_length() {
        let m = sheet();
        assert!(in_flight(&m, 0, 0), "not drawn yet: its first frame is owed");
        let start = 50 * MS;
        assert!(in_flight(&m, start, start + 399 * MS));
        assert!(!in_flight(&m, start, start + 400 * MS));
        let still = MotionLayerDesc { duration_ms: 0, ..m };
        assert!(!in_flight(&still, 0, 0));
        assert_eq!(moved_at(&still, 0, 0), Moved::REST);
    }

    /// A motion owes frames until one is drawn at rest: a frame a hair short of its end owes the
    /// next, the frame at its end owes none (the clock alone never ends a motion).
    #[test]
    fn a_motion_ends_with_a_frame_drawn_at_rest() {
        let mut tree = SurfaceTree::EMPTY;
        tree.motions[0] = sheet();
        tree.motion_count = 1;
        let start = 1_000 * MS;
        let starts = [start, 0, 0, 0];
        assert!(frame_owed(&tree, &[0; MAX_MOTION_LAYERS], 0), "not drawn yet");
        assert!(frame_owed(&tree, &starts, start + 399 * MS), "the last frame was short of rest");
        assert!(!frame_owed(&tree, &starts, start + 400 * MS), "drawn at rest: nothing owed");
        assert_eq!(moved_at(&tree.motions[0], start, start + 399 * MS).opacity, 254);
    }

    #[test]
    fn test_reject_an_unknown_motion_token() {
        let mut tree = SurfaceTree::EMPTY;
        tree.motions[0] = sheet();
        tree.motion_count = 1;
        assert!(tokens_known(&tree));
        tree.motions[0].token = MotionToken::ALL.len() as u8;
        assert!(!tokens_known(&tree), "a token past the curated set");
        assert_eq!(moved_at(&tree.motions[0], 0, 0), Moved::REST, "never drawn moved");
        assert!(!in_flight(&tree.motions[0], 0, 0));
    }

    /// The same motion declared again continues (its start carried by id); a new id starts anew,
    /// wherever in the list either stands.
    #[test]
    fn a_motion_keeps_its_start_across_structures_by_id() {
        let mut now = SurfaceTree::EMPTY;
        now.motions[0] = sheet();
        now.motions[1] = MotionLayerDesc { id: 9, x: 300, ..sheet() };
        now.motion_count = 2;
        let starts = [100, 200, 0, 0];
        let mut next = SurfaceTree::EMPTY;
        next.motions[0] = MotionLayerDesc { id: 9, x: 300, ..sheet() };
        next.motions[1] = MotionLayerDesc { id: 11, ..sheet() };
        next.motion_count = 2;
        assert_eq!(carry_starts(&now, &starts, &next), [200, 0, 0, 0]);
    }

    /// What moves with a motion layer: the regions and scroll layers it claims, a rectangle its
    /// rest holds; the ids its layers carry while moving are unique and never a window's key.
    #[test]
    fn a_motion_layer_carries_what_it_claims() {
        let mut tree = SurfaceTree::EMPTY;
        tree.motions[0] = MotionLayerDesc { regions: 0b10, scrolls: 0b1, ..sheet() };
        tree.motion_count = 1;
        assert_eq!(region_owner(&tree, 1), Some(0));
        assert_eq!(region_owner(&tree, 0), None);
        assert_eq!(scroll_owner(&tree, 0), Some(0));
        assert_eq!(holder(&tree, (30, 40, 2, 18)), Some(0), "a caret inside the sheet");
        assert_eq!(holder(&tree, (10, 40, 2, 18)), None);
        let ids = [
            layer_id(0xFFFF, 0),
            layer_id(0xFFFF, 1),
            layer_id(3, 0),
            backdrop_id(0xFFFF, 0),
            backdrop_id(0xFFFF, 3),
            layer_id(0xFFFF, 3),
        ];
        assert!(ids.iter().all(|&id| id > 0xFFFF && id != u32::MAX));
        for (i, a) in ids.iter().enumerate() {
            assert!(ids[i + 1..].iter().all(|b| a != b), "transform ids are unique: {a:#x}");
        }
    }

    /// Two layers overlapping at rest (a modal over a panel): the later one — drawn on top —
    /// holds what lies in both; what only the first covers stays the first's.
    #[test]
    fn of_two_overlapping_layers_the_later_holds_the_point() {
        let mut tree = SurfaceTree::EMPTY;
        tree.motions[0] = sheet(); // 20,30 100×200
        tree.motions[1] = MotionLayerDesc { x: 60, y: 100, w: 300, h: 100, id: 8, ..sheet() };
        tree.motion_count = 2;
        assert_eq!(holder(&tree, (70, 110, 2, 18)), Some(1), "in both: the later one");
        assert_eq!(holder(&tree, (25, 40, 2, 18)), Some(0), "in the first only");
        assert_eq!(holder(&tree, (300, 110, 2, 18)), Some(1), "in the later only");
    }
}
