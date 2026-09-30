// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: inputd's display space — the coordinate space windowd hit-tests in — asked of windowd
//! ONCE at inputd's start (RFC-0093 §5 as amended 2026-09-30, RFC-0098 C7). The mode is gpud's
//! decision; windowd receives it with the framebuffer grant and is the one peer inputd has for
//! pointer semantics, so inputd asks windowd — one call over inputd's declared reply inbox, no
//! retry, no default standing in meanwhile (the polled query this replaces is retired). The
//! answer rides the 8-byte header: `[I, N, 1, op | 0x80, w: u16 le, h: u16 le]`; a zero
//! dimension is a refusal.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below

use crate::{frame_has_op, MAGIC0, MAGIC1, VERSION};

/// inputd → windowd: "which space do you hit-test in?" (reply on the moved reply inbox).
pub const OP_GET_DISPLAY_SPACE: u8 = 5;

/// Frame length of the request and of the answer (the protocol header).
pub const DISPLAY_SPACE_FRAME_LEN: usize = 8;

/// The request frame.
#[must_use]
pub fn encode_get_display_space() -> [u8; DISPLAY_SPACE_FRAME_LEN] {
    [MAGIC0, MAGIC1, VERSION, OP_GET_DISPLAY_SPACE, 0, 0, 0, 0]
}

/// windowd's answer: its visible mode, `(0, 0)` to refuse. Dimensions above `u16::MAX` cannot
/// be a display mode here (the layout maximum is far below) and encode as a refusal.
#[must_use]
pub fn encode_display_space(width: u32, height: u32) -> [u8; DISPLAY_SPACE_FRAME_LEN] {
    let (w, h) = match (u16::try_from(width), u16::try_from(height)) {
        (Ok(w), Ok(h)) => (w, h),
        _ => (0, 0),
    };
    let [w0, w1] = w.to_le_bytes();
    let [h0, h1] = h.to_le_bytes();
    [MAGIC0, MAGIC1, VERSION, OP_GET_DISPLAY_SPACE | 0x80, w0, w1, h0, h1]
}

/// Decode windowd's answer; `None` for a malformed frame and for a refusal (a zero dimension).
#[must_use]
pub fn decode_display_space(frame: &[u8]) -> Option<(u32, u32)> {
    if frame.len() != DISPLAY_SPACE_FRAME_LEN || !frame_has_op(frame, OP_GET_DISPLAY_SPACE | 0x80) {
        return None;
    }
    let w = u32::from(u16::from_le_bytes([frame[4], frame[5]]));
    let h = u32::from(u16::from_le_bytes([frame[6], frame[7]]));
    (w != 0 && h != 0).then_some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_space_roundtrip_golden_bytes() {
        let f = encode_display_space(1920, 1080);
        assert_eq!(f, [b'I', b'N', 1, 0x85, 0x80, 0x07, 0x38, 0x04]);
        assert_eq!(decode_display_space(&f), Some((1920, 1080)));
        assert!(frame_has_op(&encode_get_display_space(), OP_GET_DISPLAY_SPACE));
    }

    /// A refusal, an oversized mode and a stranger's frame never become a display space.
    #[test]
    fn test_reject_refused_or_malformed_display_space() {
        assert_eq!(decode_display_space(&encode_display_space(0, 1080)), None);
        assert_eq!(decode_display_space(&encode_display_space(1920, 0)), None);
        assert_eq!(decode_display_space(&encode_display_space(70_000, 1080)), None);
        let f = encode_display_space(1280, 800);
        assert_eq!(decode_display_space(&f[..7]), None);
        // The REQUEST frame is not an answer.
        assert_eq!(decode_display_space(&encode_get_display_space()), None);
        let mut other = f;
        other[3] = crate::OP_GET_VISIBLE_STATE | 0x80;
        assert_eq!(decode_display_space(&other), None);
    }
}
