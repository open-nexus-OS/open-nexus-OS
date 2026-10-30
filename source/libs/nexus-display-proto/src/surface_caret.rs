// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `OP_SURFACE_CARET` (RFC-0113 §5, ADR-0090 — the compositor owns the caret): an app
//! declares its focused field's caret — a thin rectangle in surface pixels, its colour, whether it
//! blinks — and windowd draws it over the surface as a layer and blinks it on its own one-shot
//! timer: no app wake per blink, no raster, a caret move is this one small frame. `RESET` restarts
//! the blink (an edit, a move: solid for a whole period, as text fields do); without `VISIBLE` the
//! caret is gone (the focus left the field). windowd draws it only while the surface holds window
//! focus and is visible, clipped to the surface. Fail-closed decode.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (round trip, `test_reject_*` for the bounds, the flags, a
//!   foreign op)

use crate::client_surface::{has_op, header, HEADER_LEN};

/// app → windowd: the focused field's caret.
pub const OP_SURFACE_CARET: u8 = 37;

/// The caret shows (without it: no caret on this surface).
pub const CARET_VISIBLE: u8 = 1;
/// The caret blinks (a field that is not editable shows a steady one, or none).
pub const CARET_BLINK: u8 = 2;
/// The blink restarts: solid for a whole period (an edit or a move just happened).
pub const CARET_RESET: u8 = 4;
const CARET_FLAGS: u8 = CARET_VISIBLE | CARET_BLINK | CARET_RESET;

/// A caret is thin: wider than this is no caret.
pub const CARET_W_MAX: u16 = 8;
/// …and no taller than a line of the largest text a field shows.
pub const CARET_H_MAX: u16 = 256;

/// The caret an app declares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caret {
    /// Surface pixels.
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// Blue, green, red, alpha (the surface's byte order).
    pub bgra: [u8; 4],
    /// `CARET_*`.
    pub flags: u8,
}

impl Caret {
    /// No caret on the surface.
    pub const HIDDEN: Self = Self { x: 0, y: 0, w: 0, h: 0, bgra: [0; 4], flags: 0 };

    #[must_use]
    pub const fn visible(&self) -> bool {
        self.flags & CARET_VISIBLE != 0
    }

    /// Whether the declaration is one a field can make: known flags, and a visible caret is a
    /// thin, non-empty rectangle.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        if self.flags & !CARET_FLAGS != 0 {
            return false;
        }
        !self.visible()
            || (self.w > 0 && self.w <= CARET_W_MAX && self.h > 0 && self.h <= CARET_H_MAX)
    }
}

/// surface id, x, y, w, h, colour, flags.
pub const SURFACE_CARET_FRAME_LEN: usize = HEADER_LEN + 4 + 8 + 4 + 1;

/// Encodes `caret` for `surface_id`; `None` for a declaration no field can make.
#[must_use]
pub fn encode_surface_caret(
    surface_id: u32,
    caret: &Caret,
) -> Option<[u8; SURFACE_CARET_FRAME_LEN]> {
    if !caret.is_valid() {
        return None;
    }
    const RECT: usize = HEADER_LEN + 4;
    let mut f = [0u8; SURFACE_CARET_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CARET));
    f[HEADER_LEN..RECT].copy_from_slice(&surface_id.to_le_bytes());
    for (i, v) in [caret.x, caret.y, caret.w, caret.h].iter().enumerate() {
        f[RECT + i * 2..RECT + i * 2 + 2].copy_from_slice(&v.to_le_bytes());
    }
    f[RECT + 8..RECT + 12].copy_from_slice(&caret.bgra);
    f[RECT + 12] = caret.flags;
    Some(f)
}

/// `(surface_id, caret)`, or `None` for anything that is not exactly a well-formed caret frame.
#[must_use]
pub fn decode_surface_caret(frame: &[u8]) -> Option<(u32, Caret)> {
    const RECT: usize = HEADER_LEN + 4;
    if !has_op(frame, OP_SURFACE_CARET) || frame.len() != SURFACE_CARET_FRAME_LEN {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]);
    let c = &frame[RECT + 8..RECT + 12];
    let caret = Caret {
        x: u16_at(RECT),
        y: u16_at(RECT + 2),
        w: u16_at(RECT + 4),
        h: u16_at(RECT + 6),
        bgra: [c[0], c[1], c[2], c[3]],
        flags: frame[RECT + 12],
    };
    let id = &frame[HEADER_LEN..RECT];
    let surface_id = u32::from_le_bytes([id[0], id[1], id[2], id[3]]);
    caret.is_valid().then_some((surface_id, caret))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caret() -> Caret {
        Caret {
            x: 40,
            y: 12,
            w: 2,
            h: 20,
            bgra: [0x20, 0x20, 0x20, 0xff],
            flags: CARET_VISIBLE | CARET_BLINK | CARET_RESET,
        }
    }

    #[test]
    fn a_caret_round_trips_and_hides() {
        let f = encode_surface_caret(7, &caret()).expect("encodes");
        assert_eq!(decode_surface_caret(&f), Some((7, caret())));
        let gone = encode_surface_caret(7, &Caret::HIDDEN).expect("a hidden caret encodes");
        assert_eq!(decode_surface_caret(&gone).map(|(_, c)| c.visible()), Some(false));
    }

    #[test]
    fn test_reject_a_caret_no_field_draws() {
        assert_eq!(encode_surface_caret(7, &Caret { w: CARET_W_MAX + 1, ..caret() }), None);
        assert_eq!(encode_surface_caret(7, &Caret { h: 0, ..caret() }), None);
        assert_eq!(
            encode_surface_caret(7, &Caret { flags: 0x80 | CARET_VISIBLE, ..caret() }),
            None
        );
        let f = encode_surface_caret(7, &caret()).expect("encodes");
        let mut wide = f;
        let w_at = HEADER_LEN + 4 + 4;
        wide[w_at..w_at + 2].copy_from_slice(&40u16.to_le_bytes());
        assert_eq!(decode_surface_caret(&wide), None, "a wide bar is no caret, on the wire too");
        assert_eq!(decode_surface_caret(&f[..f.len() - 1]), None, "short");
        let mut other = f;
        other[3] = crate::surface_pressure::OP_SURFACE_PRESSURE;
        assert_eq!(decode_surface_caret(&other), None, "foreign op");
    }

    #[test]
    fn the_caret_op_is_its_own() {
        use crate::surface_text::{OP_SURFACE_TEXT, OP_SURFACE_TEXT_FOCUS};
        for op in [
            OP_SURFACE_TEXT,
            OP_SURFACE_TEXT_FOCUS,
            crate::surface_layers::OP_SURFACE_MOTION,
            crate::surface_seen::OP_SURFACE_SEEN,
            crate::surface_pressure::OP_SURFACE_PRESSURE,
        ] {
            assert_ne!(op, OP_SURFACE_CARET);
        }
    }
}
