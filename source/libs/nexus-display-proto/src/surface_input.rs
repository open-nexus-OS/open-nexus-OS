// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the input kinds of the client-surface protocol (ADR-0042) — what `OP_SURFACE_INPUT`
//! carries from windowd to an app surface: taps, hover motion, leave, wheel and (RFC-0095) the
//! drag gesture — and, since TASK-0346 P2 (RFC-0113 §4), what a press says beyond its place: the
//! modifiers held and its click count (a Shift+click extends a selection, a double click takes a
//! word) — and, since TASK-0346 P4 (RFC-0113 §2), what a MOVE or DRAG carries behind it: every
//! sample since the last one (`surface_motion`). Split out of `client_surface.rs` under the
//! structure ratchet; `client_surface` re-exports every live item, so its paths stay valid.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (append-only)
//! TEST_COVERAGE: unit tests below (round trip, `test_reject_*` for a frame of the wrong length)

use crate::client_surface::{has_op, header, HEADER_LEN, OP_SURFACE_INPUT};
use crate::surface_motion::{MotionBatch, MOTION_WIRE_MAX};

/// Input kinds (taps + hover motion; keys land with the focus model).
pub const INPUT_KIND_TAP: u8 = 0;
/// Frame-aligned pointer motion inside the surface (hover). windowd stages
/// raw input per frame, so MOVE volume is bounded by frame rate, not by the
/// device event rate.
pub const INPUT_KIND_MOVE: u8 = 1;
/// The pointer left the surface (or moved onto another surface/chrome):
/// the client clears any hover presentation. x/y carry the last position.
pub const INPUT_KIND_LEAVE: u8 = 2;
/// Wheel scroll over the surface: `x` carries the surface-local pointer x,
/// `y` carries the SIGNED notch delta reinterpreted as `u16` (decode with
/// `as i16` — see `wheel_delta_from_wire`). The sign is the raw evdev
/// `REL_WHEEL` convention: +1 = wheel UP (away from the user).
pub const INPUT_KIND_WHEEL: u8 = 3;
/// RETIRED (RFC-0109 / ADR-0076, value reserved forever — never reuse): the
/// compositor's scroll-position push. The app runs the one scroll physics and
/// knows its offset; windowd composes the offset the app's motion declares.
pub const INPUT_KIND_SCROLL_POS: u8 = 4;
/// Pointer motion while the primary button is held (RFC-0095 — the DSL drag gesture): sent
/// INSTEAD of `MOVE` to the surface that took the press, at the same frame-bounded rate.
/// `x`/`y` are surface-local; the press itself is the `TAP` that started the drag.
pub const INPUT_KIND_DRAG: u8 = 5;
/// The primary button was released (RFC-0095): ends a drag on the surface that took the press;
/// `x`/`y` carry the release position.
pub const INPUT_KIND_RELEASE: u8 = 6;

/// Recovers the signed wheel delta a `INPUT_KIND_WHEEL` frame carries in its
/// `y` field (the wire field is `u16`; the delta is an `i16` reinterpret).
#[must_use]
pub const fn wheel_delta_from_wire(y: u16) -> i32 {
    y as i16 as i32
}

/// The `y`-field wire encoding of a signed wheel delta (clamped to `i16`).
#[must_use]
pub const fn wheel_delta_to_wire(delta: i32) -> u16 {
    let d = if delta > i16::MAX as i32 {
        i16::MAX
    } else if delta < i16::MIN as i32 {
        i16::MIN
    } else {
        delta as i16
    };
    d as u16
}

/// The modifier bits a press carries — one definition for inputd's visible state and the surface
/// frame (TASK-0346 P2).
pub const MOD_SHIFT: u8 = 1;
pub const MOD_CTRL: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_ALTGR: u8 = 8;
pub const MOD_SUPER: u8 = 16;
const MOD_KNOWN: u8 = MOD_SHIFT | MOD_CTRL | MOD_ALT | MOD_ALTGR | MOD_SUPER;

/// The most clicks a press counts (a triple click; more stay three).
pub const CLICKS_MAX: u8 = 3;

/// What a pointer frame says beyond its place: the modifiers held (`MOD_*`) and, for a press,
/// its click count (1–3; 0 for anything that is not a press).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerFacts {
    pub modifiers: u8,
    pub clicks: u8,
}

/// `OP_SURFACE_INPUT`: header, surface, kind, x, y, modifiers, clicks.
pub const SURFACE_INPUT_FRAME_LEN: usize = HEADER_LEN + 11;

/// A pointer frame with no facts (motion, leave, wheel, drag, release).
#[must_use]
pub fn encode_surface_input(
    surface_id: u32,
    kind: u8,
    x: u16,
    y: u16,
) -> [u8; SURFACE_INPUT_FRAME_LEN] {
    encode_surface_pointer(surface_id, kind, x, y, PointerFacts::default())
}

/// A pointer frame with its facts (a press: the modifiers held, the click count).
#[must_use]
pub fn encode_surface_pointer(
    surface_id: u32,
    kind: u8,
    x: u16,
    y: u16,
    facts: PointerFacts,
) -> [u8; SURFACE_INPUT_FRAME_LEN] {
    let mut f = [0u8; SURFACE_INPUT_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_INPUT));
    f[4..8].copy_from_slice(&surface_id.to_le_bytes());
    f[8] = kind;
    f[9..11].copy_from_slice(&x.to_le_bytes());
    f[11..13].copy_from_slice(&y.to_le_bytes());
    f[13] = facts.modifiers & MOD_KNOWN;
    f[14] = facts.clicks.min(CLICKS_MAX);
    f
}

/// `(surface_id, kind, x, y, facts)` — `None` for a frame of another op or length, or with
/// modifier bits or a click count no press can have (fail-closed).
#[must_use]
pub fn decode_surface_input(frame: &[u8]) -> Option<(u32, u8, u16, u16, PointerFacts)> {
    if !has_op(frame, OP_SURFACE_INPUT) || frame.len() != SURFACE_INPUT_FRAME_LEN {
        return None;
    }
    let facts = PointerFacts { modifiers: frame[13], clicks: frame[14] };
    if facts.modifiers & !MOD_KNOWN != 0 || facts.clicks > CLICKS_MAX {
        return None;
    }
    Some((
        u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]),
        frame[8],
        u16::from_le_bytes([frame[9], frame[10]]),
        u16::from_le_bytes([frame[11], frame[12]]),
        facts,
    ))
}

/// A MOVE or DRAG with its motion (RFC-0113 §2): the pointer frame (x/y the newest position)
/// followed by every sample since the surface's last one.
pub const SURFACE_MOTION_FRAME_MAX: usize = SURFACE_INPUT_FRAME_LEN + MOTION_WIRE_MAX;
const _: () = assert!(SURFACE_MOTION_FRAME_MAX <= crate::surface_text::CLIENT_EVENT_FRAME_MAX);

/// A MOVE or DRAG frame with `motion` behind it; `None` for another kind.
#[must_use]
pub fn encode_surface_motion(
    surface_id: u32,
    kind: u8,
    x: u16,
    y: u16,
    motion: &MotionBatch,
) -> Option<([u8; SURFACE_MOTION_FRAME_MAX], usize)> {
    if kind != INPUT_KIND_MOVE && kind != INPUT_KIND_DRAG {
        return None;
    }
    let mut f = [0u8; SURFACE_MOTION_FRAME_MAX];
    f[..SURFACE_INPUT_FRAME_LEN].copy_from_slice(&encode_surface_input(surface_id, kind, x, y));
    let n = motion.encode_into(&mut f[SURFACE_INPUT_FRAME_LEN..])?;
    Some((f, SURFACE_INPUT_FRAME_LEN + n))
}

/// `(surface_id, kind, x, y, motion)` of a MOVE or DRAG frame with its motion — `None` for any
/// other frame, a press's facts on it, or a motion that does not exactly fill the rest.
#[must_use]
pub fn decode_surface_motion(frame: &[u8]) -> Option<(u32, u8, u16, u16, MotionBatch)> {
    let (head, rest) = frame.split_at_checked(SURFACE_INPUT_FRAME_LEN)?;
    let (surface, kind, x, y, facts) = decode_surface_input(head)?;
    if (kind != INPUT_KIND_MOVE && kind != INPUT_KIND_DRAG) || facts != PointerFacts::default() {
        return None;
    }
    Some((surface, kind, x, y, MotionBatch::decode(rest)?))
}

/// Two motion frames of one surface and kind, `older` then `newer`, as ONE frame: `newer`'s
/// position, `older`'s samples before `newer`'s (a client's outbox replaces a waiting MOVE or
/// DRAG with the next one — the samples stay). `None` when they do not join.
#[must_use]
pub fn join_motion_frames(
    older: &[u8],
    newer: &[u8],
) -> Option<([u8; SURFACE_MOTION_FRAME_MAX], usize)> {
    let (surface, kind, _, _, mut motion) = decode_surface_motion(older)?;
    let (surface_n, kind_n, x, y, later) = decode_surface_motion(newer)?;
    if surface != surface_n || kind != kind_n {
        return None;
    }
    motion.append(&later);
    encode_surface_motion(surface, kind, x, y, &motion)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface_motion::MotionSample;

    fn motion(from: u32, n: u32) -> MotionBatch {
        let mut batch = MotionBatch::new();
        for i in from..from + n {
            batch.push(MotionSample {
                x: i as i32,
                y: 2,
                t_ns: 1_000_000 * u64::from(i + 1),
                seq: i,
            });
        }
        batch
    }

    #[test]
    fn a_move_carries_its_motion_and_two_join() {
        let (f, n) = encode_surface_motion(4, INPUT_KIND_DRAG, 30, 2, &motion(0, 3)).expect("drag");
        assert_eq!(decode_surface_motion(&f[..n]), Some((4, INPUT_KIND_DRAG, 30, 2, motion(0, 3))));
        assert_eq!(decode_surface_input(&f[..n]), None, "the fixed frame's decoder refuses it");
        let (g, m) = encode_surface_motion(4, INPUT_KIND_DRAG, 33, 2, &motion(3, 2)).expect("drag");
        let (j, k) = join_motion_frames(&f[..n], &g[..m]).expect("joins");
        assert_eq!(decode_surface_motion(&j[..k]), Some((4, INPUT_KIND_DRAG, 33, 2, motion(0, 5))));
        let (h, l) = encode_surface_motion(5, INPUT_KIND_DRAG, 1, 1, &motion(9, 1)).expect("drag");
        assert_eq!(join_motion_frames(&f[..n], &h[..l]), None, "another surface");
    }

    #[test]
    fn test_reject_a_motion_frame_of_another_kind_or_with_garbage() {
        assert!(encode_surface_motion(4, INPUT_KIND_TAP, 1, 1, &motion(0, 1)).is_none());
        let (f, n) = encode_surface_motion(4, INPUT_KIND_MOVE, 1, 1, &motion(0, 2)).expect("move");
        assert_eq!(decode_surface_motion(&f[..n + 1]), None, "trailing bytes");
        assert_eq!(decode_surface_motion(&f[..SURFACE_INPUT_FRAME_LEN]), None, "no motion");
        let tap = encode_surface_input(4, INPUT_KIND_TAP, 1, 1);
        assert_eq!(decode_surface_motion(&tap), None);
    }

    #[test]
    fn a_press_carries_its_modifiers_and_click_count() {
        let facts = PointerFacts { modifiers: MOD_SHIFT | MOD_CTRL, clicks: 2 };
        let f = encode_surface_pointer(9, INPUT_KIND_TAP, 120, 88, facts);
        assert_eq!(decode_surface_input(&f), Some((9, INPUT_KIND_TAP, 120, 88, facts)));
        let plain = encode_surface_input(9, INPUT_KIND_MOVE, 1, 2);
        assert_eq!(decode_surface_input(&plain).map(|d| d.4), Some(PointerFacts::default()));
        let many =
            encode_surface_pointer(9, INPUT_KIND_TAP, 0, 0, PointerFacts { clicks: 9, ..facts });
        assert_eq!(decode_surface_input(&many).map(|d| d.4.clicks), Some(CLICKS_MAX));
    }

    #[test]
    fn test_reject_a_pointer_frame_of_the_wrong_length_or_unknown_facts() {
        let f = encode_surface_input(9, INPUT_KIND_TAP, 1, 2);
        assert_eq!(decode_surface_input(&f[..f.len() - 2]), None, "the pre-facts length");
        let mut bad = f;
        bad[13] = 0x80;
        assert_eq!(decode_surface_input(&bad), None, "an unknown modifier bit");
        let mut bad = f;
        bad[14] = 4;
        assert_eq!(decode_surface_input(&bad), None, "four clicks");
    }

    #[test]
    fn wheel_deltas_round_trip_and_clamp() {
        for d in [-3, -1, 0, 1, 7] {
            assert_eq!(wheel_delta_from_wire(wheel_delta_to_wire(d)), d);
        }
        assert_eq!(wheel_delta_from_wire(wheel_delta_to_wire(100_000)), i32::from(i16::MAX));
        assert_eq!(wheel_delta_from_wire(wheel_delta_to_wire(-100_000)), i32::from(i16::MIN));
    }

    #[test]
    fn input_kinds_are_distinct() {
        let kinds = [
            INPUT_KIND_TAP,
            INPUT_KIND_MOVE,
            INPUT_KIND_LEAVE,
            INPUT_KIND_WHEEL,
            INPUT_KIND_SCROLL_POS,
            INPUT_KIND_DRAG,
            INPUT_KIND_RELEASE,
        ];
        for (i, a) in kinds.iter().enumerate() {
            assert!(kinds[i + 1..].iter().all(|b| b != a), "kind {a} twice");
        }
    }
}
