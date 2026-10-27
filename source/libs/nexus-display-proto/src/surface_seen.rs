// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `OP_SURFACE_SEEN` (TASK-0333 6e-1, RFC-0100 §6, ADR-0086): the compositor tells an
//! app's own window what it sees of it — focused, visible, hidden — the same sighting it states
//! to the lifecycle authority (ADR-0083). The app acts on `hidden` alone: it writes its
//! `@persist` snapshot while the grace runs, before the authority freezes it, so a kill under
//! pressure (memd) loses nothing the user typed into a field marked durable. One byte, one
//! direction (compositor → client), fail-closed decode.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (round trip, short/foreign/unknown frames rejected)

use crate::client_surface::{has_op, header, HEADER_LEN};

/// The compositor's sighting of the receiving window.
pub const OP_SURFACE_SEEN: u8 = 35;

pub const SURFACE_SEEN_FRAME_LEN: usize = HEADER_LEN + 1;

/// The window has focus (the user is in it).
pub const SEEN_FOCUSED: u8 = 0;
/// On screen, not focused.
pub const SEEN_VISIBLE: u8 = 1;
/// Minimized — nothing of the window is on screen; the lifecycle grace runs.
pub const SEEN_HIDDEN: u8 = 2;

#[must_use]
pub fn encode_surface_seen(seen: u8) -> [u8; SURFACE_SEEN_FRAME_LEN] {
    let mut f = [0u8; SURFACE_SEEN_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_SEEN));
    f[HEADER_LEN] = seen;
    f
}

/// The sighting, or `None` for a frame that is not a well-formed seen (another op, a short or a
/// long frame, a value this build does not know).
#[must_use]
pub fn decode_surface_seen(frame: &[u8]) -> Option<u8> {
    if !has_op(frame, OP_SURFACE_SEEN) || frame.len() != SURFACE_SEEN_FRAME_LEN {
        return None;
    }
    let seen = frame[HEADER_LEN];
    (seen <= SEEN_HIDDEN).then_some(seen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_three_sightings() {
        for seen in [SEEN_FOCUSED, SEEN_VISIBLE, SEEN_HIDDEN] {
            assert_eq!(decode_surface_seen(&encode_surface_seen(seen)), Some(seen));
        }
    }

    #[test]
    fn test_reject_short_foreign_or_unknown_seen() {
        let f = encode_surface_seen(SEEN_HIDDEN);
        assert_eq!(decode_surface_seen(&f[..HEADER_LEN]), None, "short");
        let mut long = [0u8; SURFACE_SEEN_FRAME_LEN + 1];
        long[..SURFACE_SEEN_FRAME_LEN].copy_from_slice(&f);
        assert_eq!(decode_surface_seen(&long), None, "long");
        let mut other = f;
        other[3] = crate::client_surface::OP_SURFACE_FRAME;
        assert_eq!(decode_surface_seen(&other), None, "foreign op");
        assert_eq!(decode_surface_seen(&encode_surface_seen(3)), None, "gone is nobody's frame");
        assert_eq!(decode_surface_seen(&encode_surface_seen(0xff)), None, "unknown value");
    }
}
