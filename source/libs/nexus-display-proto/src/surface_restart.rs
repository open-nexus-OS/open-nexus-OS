// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The two frames that tell an app's window why its event channel went quiet (RFC-0112,
//! amendment 2026-11-02, TASK-0348 Phase 6). `OP_SURFACE_CLOSED`: the compositor closed the
//! window ON PURPOSE (the user, the window manager) and says so before it lets go of the channel
//! — the app saves its record and ends. `OP_COMPOSITOR_BACK`: the compositor died and a new one
//! runs — execd, the app's parent, sends it on the app's channel with a fresh SEND of that
//! channel moved along (the app gave its last one to the dead compositor); the app attaches the
//! channel again and draws. An EOF on the channel without a CLOSED before it is the compositor's
//! death. Header only, one direction each, fail-closed decode.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (round trips; other ops and long frames refused)

use crate::client_surface::{has_op, header, HEADER_LEN};

/// The compositor closed the window on purpose (compositor → app, before the channel goes).
pub const OP_SURFACE_CLOSED: u8 = 39;
/// A new compositor runs (execd → app, a fresh SEND of the app's event channel moved along).
pub const OP_COMPOSITOR_BACK: u8 = 40;

/// Both frames are the bare header.
pub const SURFACE_RESTART_FRAME_LEN: usize = HEADER_LEN;

#[must_use]
pub fn encode_surface_closed() -> [u8; SURFACE_RESTART_FRAME_LEN] {
    header(OP_SURFACE_CLOSED)
}

#[must_use]
pub fn encode_compositor_back() -> [u8; SURFACE_RESTART_FRAME_LEN] {
    header(OP_COMPOSITOR_BACK)
}

/// Whether `frame` is a well-formed `OP_SURFACE_CLOSED`.
#[must_use]
pub fn is_surface_closed(frame: &[u8]) -> bool {
    has_op(frame, OP_SURFACE_CLOSED) && frame.len() == SURFACE_RESTART_FRAME_LEN
}

/// Whether `frame` is a well-formed `OP_COMPOSITOR_BACK`.
#[must_use]
pub fn is_compositor_back(frame: &[u8]) -> bool {
    has_op(frame, OP_COMPOSITOR_BACK) && frame.len() == SURFACE_RESTART_FRAME_LEN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_frames_round_trip() {
        assert!(is_surface_closed(&encode_surface_closed()));
        assert!(is_compositor_back(&encode_compositor_back()));
        assert!(!is_surface_closed(&encode_compositor_back()));
        assert!(!is_compositor_back(&encode_surface_closed()));
    }

    #[test]
    fn test_reject_a_long_or_foreign_frame() {
        let mut long = [0u8; SURFACE_RESTART_FRAME_LEN + 1];
        long[..HEADER_LEN].copy_from_slice(&encode_surface_closed());
        assert!(!is_surface_closed(&long), "trailing bytes");
        assert!(!is_compositor_back(&header(crate::surface_pressure::OP_SURFACE_PRESSURE)));
        assert!(!is_compositor_back(&[]), "empty");
    }
}
