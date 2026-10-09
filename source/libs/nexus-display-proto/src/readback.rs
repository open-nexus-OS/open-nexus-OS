// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the ONE pixel readback (RFC-0095, ADR-0071) — windowd → gpud
//! `[OP_READBACK][x:u16][y:u16][w:u16][h:u16][flags]` with the destination VMO moved in. gpud
//! writes the rectangle of the frame the display shows into the VMO, rows tight (`w × 4`, BGRA).
//! On GL the front render target is copied into a resource backed by the moved VMO and
//! transferred back — the scanout is never a transfer source (RFC-0093 §5); on the CPU paths
//! the display plane is copied. `READBACK_FREEZE` also makes the frame the compositor's base
//! layer until windowd thaws it. Reply: `[status, OP_READBACK]` — two bytes, a shape windowd
//! never reads as a cursor status (one byte) or a present ack (five).
//!
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (append-only)
//! TEST_COVERAGE: unit tests below

/// windowd → gpud: read a rectangle of the shown frame.
pub const OP_READBACK: u8 = 15;
/// Request length.
pub const READBACK_FRAME_LEN: usize = 10;
/// Reply length: `[status, OP_READBACK]`.
pub const READBACK_REPLY_LEN: usize = 2;
/// The frame also becomes the base layer (the freeze).
pub const READBACK_FREEZE: u8 = 1;

/// A readback request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Readback {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub flags: u8,
}

impl Readback {
    /// The bytes the destination VMO must hold (rows tight, BGRA).
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.w as usize * self.h as usize * 4
    }

    /// Whether the rectangle is non-empty and inside a `display_w × display_h` frame.
    #[must_use]
    pub fn fits(&self, display_w: u32, display_h: u32) -> bool {
        self.w != 0
            && self.h != 0
            && u32::from(self.x) + u32::from(self.w) <= display_w
            && u32::from(self.y) + u32::from(self.h) <= display_h
    }
}

/// Encodes a request.
#[must_use]
pub fn encode_readback(req: &Readback) -> [u8; READBACK_FRAME_LEN] {
    let mut f = [0u8; READBACK_FRAME_LEN];
    f[0] = OP_READBACK;
    f[1..3].copy_from_slice(&req.x.to_le_bytes());
    f[3..5].copy_from_slice(&req.y.to_le_bytes());
    f[5..7].copy_from_slice(&req.w.to_le_bytes());
    f[7..9].copy_from_slice(&req.h.to_le_bytes());
    f[9] = req.flags;
    f
}

/// Fail-closed decode: exact length, no unknown flag bits.
#[must_use]
pub fn decode_readback(frame: &[u8]) -> Option<Readback> {
    if frame.len() != READBACK_FRAME_LEN || frame[0] != OP_READBACK {
        return None;
    }
    let flags = frame[9];
    if flags & !READBACK_FREEZE != 0 {
        return None;
    }
    let u16_at = |i: usize| u16::from_le_bytes([frame[i], frame[i + 1]]);
    Some(Readback { x: u16_at(1), y: u16_at(3), w: u16_at(5), h: u16_at(7), flags })
}

/// The reply.
#[must_use]
pub const fn encode_readback_reply(status: u8) -> [u8; READBACK_REPLY_LEN] {
    [status, OP_READBACK]
}

/// The reply's status, when `reply` is a readback reply.
#[must_use]
pub fn decode_readback_reply(reply: &[u8]) -> Option<u8> {
    (reply.len() == READBACK_REPLY_LEN && reply[1] == OP_READBACK).then_some(reply[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_replies_round_trip() {
        let req = Readback { x: 10, y: 20, w: 1280, h: 800, flags: READBACK_FREEZE };
        assert_eq!(decode_readback(&encode_readback(&req)), Some(req));
        assert_eq!(req.bytes(), 1280 * 800 * 4);
        assert_eq!(decode_readback_reply(&encode_readback_reply(3)), Some(3));
    }

    #[test]
    fn a_rectangle_fits_inside_the_frame_only() {
        let full = Readback { x: 0, y: 0, w: 1280, h: 800, flags: 0 };
        assert!(full.fits(1280, 800));
        assert!(!Readback { x: 1, ..full }.fits(1280, 800), "one pixel past the edge");
        assert!(!Readback { w: 0, ..full }.fits(1280, 800), "empty");
        let far = Readback { x: u16::MAX, y: 0, w: u16::MAX, h: 1, flags: 0 };
        assert!(!far.fits(1280, 800), "no wrap on huge coordinates");
    }

    #[test]
    fn test_reject_unknown_flags_lengths_and_shapes() {
        let mut f = encode_readback(&Readback { x: 0, y: 0, w: 1, h: 1, flags: 0 });
        f[9] = 0x80;
        assert_eq!(decode_readback(&f), None, "unknown flag bit");
        assert_eq!(decode_readback(&f[..9]), None, "truncated");
        assert_eq!(decode_readback_reply(&[0]), None, "a cursor status is not a readback reply");
        assert_eq!(decode_readback_reply(&[0, 1, 0, 0, 0]), None, "a present ack is not one");
    }
}
