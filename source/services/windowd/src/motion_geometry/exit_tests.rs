// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: host tests of a motion layer's exit and turn (TASK-0354 P9): a close plays every
//! enter token back toward where it came from, a re-open turns without a jump in place or
//! velocity, a backdrop fades out from where it is, a turn's state is bounded, and an
//! uninterrupted enter samples exactly as before exits existed.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::turn::{turn_from, Q_MAX, Q_MIN, V_MAX};
use super::*;
use animation::{AnimProp, MotionCategory};

const MS: u64 = 1_000_000;
const S: u64 = 1_000 * MS;

/// A layer of `token` over `ms`, starting where the token's entrance starts (transparent).
fn layer(token: MotionToken, ms: u16) -> MotionLayerDesc {
    MotionLayerDesc {
        x: 40,
        y: 60,
        w: 200,
        h: 120,
        id: 5,
        token: token.id(),
        duration_ms: ms,
        from_dx: token.resting(AnimProp::TranslateX, false) as i16,
        from_dy: token.resting(AnimProp::TranslateY, false) as i16,
        from_opacity: 0,
        from_scale: (token.resting(AnimProp::ScaleX, false) * 10_000.0) as u16,
        ..MotionLayerDesc::EMPTY
    }
}

/// A one-layer structure declaring `m`.
fn tree_of(m: MotionLayerDesc) -> SurfaceTree {
    let mut tree = SurfaceTree::EMPTY;
    tree.motions[0] = m;
    tree.motion_count = 1;
    tree
}

/// A close's segment, first drawn at `start`, from a settled enter.
const fn exit_at(start: u64) -> Segment {
    Segment { start, exit: true, turn: None, b0: 255 }
}

fn near(a: u32, b: u32) -> bool {
    a.abs_diff(b) <= 1
}

/// TASK-0354 P9: a close plays the token's own curve back toward its from-state, `q = 1 − E(p)`
/// — for every enter token, at 0 / 10 / 50 / 100 % of its length: where it rested first, the
/// offset, opacity and scale it came from growing back by `E(p)`, and at its end its from-state
/// at opacity 0.
#[test]
fn an_exit_plays_its_token_toward_where_it_came_from() {
    let enters = MotionToken::ALL.iter().filter(|t| t.category() == MotionCategory::Transition);
    let mut seen = 0;
    for &token in enters {
        seen += 1;
        let m = layer(token, 300);
        let seg = exit_at(S);
        assert_eq!(moved_at(&m, &seg, S), Moved::REST, "{token:?} 0 %: where it rested");
        for p in [0.1f32, 0.5] {
            let at = S + (300.0 * p) as u64 * MS;
            let e = token.easing().ease(p);
            let got = moved_at(&m, &seg, at);
            let from_scale = m.start_scale() as f32;
            let scale = from_scale + (SCALE_ONE as f32 - from_scale) * (1.0 - e);
            let want_dy = (f32::from(m.from_dy) * e) as i32;
            let want_dx = (f32::from(m.from_dx) * e) as i32;
            assert!((got.dy - want_dy).abs() <= 1, "{token:?} {p}: dy {} vs {want_dy}", got.dy);
            assert!((got.dx - want_dx).abs() <= 1, "{token:?} {p}: dx {} vs {want_dx}", got.dx);
            let want_op = (255.0 * (1.0 - e)).clamp(0.0, 255.0) as u32;
            assert!(near(got.opacity, want_op), "{token:?} {p}: op {} vs {want_op}", got.opacity);
            let want_scale = scale.clamp(100.0, 40_000.0) as u32;
            assert!(near(got.scale, want_scale), "{token:?} {p}: scale {}", got.scale);
        }
        let end = moved_at(&m, &seg, S + 300 * MS);
        let from = Moved {
            dx: i32::from(m.from_dx),
            dy: i32::from(m.from_dy),
            opacity: 0,
            scale: m.start_scale(),
        };
        assert_eq!(end, from, "{token:?} 100 %: its from-state, not drawn");
        assert!(!in_flight(&m, &seg, S + 300 * MS), "{token:?}: past its end it owes nothing");
    }
    assert_eq!(seen, 6, "fade, slideUp, fadeScale, slideDown, pushInline, menuIn");
}

/// TASK-0354 P9: a close re-opened mid-way turns at the frame's shown time: the same place
/// (Δq = 0, the same pixels) and the velocity it had (|Δv| < 2 % of the motion's peak), then the
/// enter settles at its rest at its length.
#[test]
fn a_reopened_close_keeps_its_place_and_velocity() {
    let token = MotionToken::SlideUp;
    let open = layer(token, 300);
    let close = MotionLayerDesc { exit: true, ..open };
    let (open_tree, close_tree) = (tree_of(open), tree_of(close));
    let mut states = [MotionState::EMPTY; MAX_MOTION_LAYERS];
    states[0] = MotionState::started(S);
    let e = 3 * S;
    states = carry(&open_tree, &states, &close_tree);
    assert_eq!(stamp(&close, &mut states[0], e, FrameNote::default()), Said::Started);
    let leaving = states[0].seg;
    assert_eq!(leaving, exit_at(e), "the enter was over: the close's design curve");
    let r = e + 120 * MS;
    states = carry(&close_tree, &states, &open_tree);
    assert_eq!(stamp(&open, &mut states[0], r, FrameNote::default()), Said::Started);
    let turned = states[0].seg;
    assert!(turned.turn.is_some(), "the close still moved: a turn");
    let q = |seg: &Segment, at: u64| progress(token, &open, seg, at).unwrap_or(1.0);
    assert_eq!(q(&leaving, r), q(&turned, r), "Δq = 0");
    assert_eq!(moved_at(&close, &leaving, r), moved_at(&open, &turned, r), "the same pixels");
    let h = 100_000; // 0.1 ms
    let v_before = (q(&leaving, r) - q(&leaving, r - h)) * 1e9 / h as f32;
    let v_after = (q(&turned, r + h) - q(&turned, r)) * 1e9 / h as f32;
    let peak = (0..300)
        .map(|i| (q(&leaving, e + (i + 1) * MS) - q(&leaving, e + i * MS)).abs() * 1e3)
        .fold(0.0f32, f32::max);
    assert!(v_before < 0.0, "the close moved toward its from-state: {v_before}");
    assert!(
        (v_after - v_before).abs() < 0.02 * peak,
        "Δv {} of peak {peak} (before {v_before}, after {v_after})",
        (v_after - v_before).abs()
    );
    assert!(q(&turned, r + 299 * MS) > 0.998, "settled to ~0.1 % at its length");
    assert_eq!(moved_at(&open, &turned, r + 300 * MS), Moved::REST, "snapped at its length");
}

/// TASK-0354 P9: a close of a modal whose scrim still fades in fades the scrim out from where it
/// is (no jump to fully drawn), down to transparent over its own length; one after a settled
/// enter fades from fully drawn.
#[test]
fn a_backdrop_fades_out_from_where_it_is() {
    let open = MotionLayerDesc {
        backdrop: 1,
        backdrop_opacity: 102,
        backdrop_ms: 280,
        ..layer(MotionToken::FadeScale, 200)
    };
    let close = MotionLayerDesc { exit: true, ..open };
    let mut states = [MotionState::EMPTY; MAX_MOTION_LAYERS];
    states[0] = MotionState::started(S);
    let c = S + 140 * MS;
    let half = backdrop_at(&open, &states[0].seg, c);
    assert_eq!(half, 197, "half its fade-in");
    states = carry(&tree_of(open), &states, &tree_of(close));
    assert_eq!(stamp(&close, &mut states[0], c, FrameNote::default()), Said::Started);
    let seg = states[0].seg;
    assert_eq!(seg.b0, 197);
    assert_eq!(backdrop_at(&close, &seg, c), 197, "the close's first frame: where it was");
    let mut last = 197;
    for i in 1..=28 {
        let b = backdrop_at(&close, &seg, c + i * 10 * MS);
        assert!(b <= last, "it only fades out: {b} after {last}");
        last = b;
    }
    assert_eq!(last, 0, "transparent at its length");
    assert_eq!(backdrop_at(&close, &seg, c + 5_000 * MS), 0, "and after");
    assert!(in_flight(&close, &seg, c + 279 * MS), "a frame is owed while the scrim fades");
    let settled = exit_at(3 * S);
    assert_eq!(backdrop_at(&close, &settled, 3 * S), 255, "after a settled enter: from drawn");
    let none = MotionLayerDesc { backdrop_ms: 0, ..close };
    assert_eq!(backdrop_at(&none, &settled, 3 * S), 0, "of no length: gone at once");
}

/// A turn's state is bounded: a flip of a 1 ms motion at its start (a finite difference of
/// 1000/s) turns at `V_MAX`; a turned segment from the most extreme state stays inside
/// [`Q_MIN`, `Q_MAX`] at every time.
#[test]
fn test_reject_an_unbounded_turn_velocity() {
    let blink = layer(MotionToken::Fade, 1);
    assert_eq!(turn_from(&blink, &Segment::enter_at(S), S), Some((0.0, V_MAX)));
    let close = Segment { start: S, exit: true, turn: None, b0: 255 };
    assert_eq!(turn_from(&blink, &close, S), Some((1.0, -V_MAX)));
    for token in MotionToken::ALL {
        let m = layer(token, 1_000);
        for (exit, q0, v0) in [(false, Q_MAX, V_MAX), (true, Q_MIN, -V_MAX), (false, Q_MIN, V_MAX)]
        {
            let seg = Segment { start: S, exit, turn: Some((q0, v0)), b0: 0 };
            for i in 0..=1_000 {
                let q = progress(token, &m, &seg, S + i * MS).unwrap_or(seg_end(exit));
                assert!((Q_MIN..=Q_MAX).contains(&q), "{token:?} {q0}/{v0}: q {q} at {i} ms");
                let ahead = turn_from(&m, &seg, S + i * MS);
                assert!(ahead.is_none_or(|(q, v)| (Q_MIN..=Q_MAX).contains(&q) && v.abs() <= V_MAX));
            }
        }
    }
}

fn seg_end(exit: bool) -> f32 {
    if exit {
        0.0
    } else {
        1.0
    }
}

/// The motion as it was evaluated before exits (TASK-0354 P2/P3), verbatim.
fn moved_before(m: &MotionLayerDesc, start: u64, at: u64) -> Moved {
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

/// The backdrop as it was evaluated before exits, verbatim.
fn backdrop_before(m: &MotionLayerDesc, start: u64, at: u64) -> u32 {
    let len = u64::from(m.backdrop_ms) * 1_000_000;
    if len == 0 || MotionToken::from_id(m.token).is_none() {
        return 255;
    }
    let t = if start == 0 { 0.0 } else { at.saturating_sub(start) as f32 / len as f32 };
    (255.0 * MotionToken::Fade.easing().ease(t)).clamp(0.0, 255.0) as u32
}

/// An enter nobody interrupts samples exactly as before — every token, every millisecond, its
/// layer and its backdrop; the curated curves never reach the clamp of `q`.
#[test]
fn an_uninterrupted_enter_samples_as_before() {
    for token in MotionToken::ALL {
        for (ms, from_op) in [(300u16, 0u8), (500, 77), (1, 255)] {
            let m = MotionLayerDesc {
                from_opacity: from_op,
                backdrop: 1,
                backdrop_ms: 280,
                from_dx: -16,
                ..layer(token, ms)
            };
            for start in [0, S] {
                let seg = Segment::enter_at(start);
                for i in 0..=u64::from(ms.max(280)) + 10 {
                    let at = S + i * MS;
                    assert_eq!(
                        moved_at(&m, &seg, at),
                        moved_before(&m, start, at),
                        "{token:?} {i}"
                    );
                    assert_eq!(backdrop_at(&m, &seg, at), backdrop_before(&m, start, at));
                }
            }
        }
        for i in 0..=10_000 {
            let e = token.easing().ease(i as f32 / 10_000.0);
            assert!((Q_MIN..=Q_MAX).contains(&e) && (Q_MIN..=Q_MAX).contains(&(1.0 - e)));
        }
    }
}

/// A leaving layer holds no node, so no caret: a caret under it moves with nothing.
#[test]
fn a_leaving_layer_holds_no_caret() {
    let m = layer(MotionToken::SlideUp, 300);
    assert_eq!(holder(&tree_of(m), (50, 70, 2, 18)), Some(0));
    let leaving = MotionLayerDesc { exit: true, ..m };
    assert_eq!(holder(&tree_of(leaving), (50, 70, 2, 18)), None);
}
