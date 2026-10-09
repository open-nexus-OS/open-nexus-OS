// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the input kinds of the client-surface protocol (ADR-0042) — what `OP_SURFACE_INPUT`
//! carries from windowd to an app surface: taps, hover motion, leave, wheel, the compositor's
//! scroll position and (RFC-0095) the drag gesture. Split out of `client_surface.rs` under the
//! structure ratchet; `client_surface` re-exports every item, so its paths stay valid.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (append-only)
//! TEST_COVERAGE: unit tests below

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
/// Compositor scroll position push (windowd → app): `y` carries the resolved
/// ABSOLUTE scroll offset in rows. Sent when windowd owns the scroll (WebRender
/// path: it shifts the layer `src_row` itself) so the app can keep its hit-test
/// + EndReached state in sync WITHOUT re-rendering on every notch.
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

#[cfg(test)]
mod tests {
    use super::*;

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
