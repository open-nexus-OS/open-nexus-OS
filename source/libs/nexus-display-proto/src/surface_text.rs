// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0075 text ops on the windowd↔app push channel — committed/
//! preedit text delivery to the focused surface (`OP_SURFACE_TEXT`) and the
//! app's widget-focus announcement (`OP_SURFACE_TEXT_FOCUS`). Same `'I','N'`
//! envelope + op space as `client_surface` (collision-pinned there).
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0146 / RFC-0075 Phase 0)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (roundtrips + reject paths)

use crate::client_surface::{has_op, header, HEADER_LEN};

/// windowd → app: composed text delivery to the FOCUSED surface (RFC-0075).
/// imed pushes commits/preedit to windowd; windowd routes them here — apps
/// must accept text only on this established push channel, never from peers.
/// Payload: `kind:u8, aux:u8, len:u8, text[..64]` (UTF-8, bounded).
pub const OP_SURFACE_TEXT: u8 = 21;
/// app → windowd: widget text-focus announcement (RFC-0075). The app owns
/// widget focus; windowd relays the aggregate to imed + inputd. Payload:
/// `focused:u8, field_kind:u8, caret x/y/w/h:u16×4` (surface coordinates —
/// the caret rect anchors the OSK / candidate strip).
pub const OP_SURFACE_TEXT_FOCUS: u8 = 22;
// Op 23 is reserved: OP_SURFACE_REGION (locale/tz/hour-format push, RFC-0077).

/// `OP_SURFACE_TEXT` kind: committed text in the payload.
pub const SURFACE_TEXT_COMMIT: u8 = 0;
/// `OP_SURFACE_TEXT` kind: preedit snapshot (aux = caret index; empty clears).
pub const SURFACE_TEXT_PREEDIT: u8 = 1;
/// `OP_SURFACE_TEXT` kind: editing action passed through composition
/// (aux = imed wire `ACTION_*`; payload empty).
pub const SURFACE_TEXT_ACTION: u8 = 2;

/// Maximum UTF-8 payload bytes per `OP_SURFACE_TEXT` frame (RFC-0075 bound).
pub const SURFACE_TEXT_MAX_BYTES: usize = 64;
pub const SURFACE_TEXT_FRAME_MAX: usize = HEADER_LEN + 3 + SURFACE_TEXT_MAX_BYTES;

/// Encodes a text push; `None` when `text` exceeds the bound.
#[must_use]
pub fn encode_surface_text(
    kind: u8,
    aux: u8,
    text: &str,
) -> Option<([u8; SURFACE_TEXT_FRAME_MAX], usize)> {
    let bytes = text.as_bytes();
    if bytes.len() > SURFACE_TEXT_MAX_BYTES {
        return None;
    }
    let mut f = [0u8; SURFACE_TEXT_FRAME_MAX];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_TEXT));
    f[4] = kind;
    f[5] = aux;
    f[6] = bytes.len() as u8;
    f[7..7 + bytes.len()].copy_from_slice(bytes);
    Some((f, HEADER_LEN + 3 + bytes.len()))
}

/// `(kind, aux, text)` — fail-closed on truncation, oversize or invalid UTF-8.
#[must_use]
pub fn decode_surface_text(frame: &[u8]) -> Option<(u8, u8, &str)> {
    if !has_op(frame, OP_SURFACE_TEXT) || frame.len() < HEADER_LEN + 3 {
        return None;
    }
    let len = usize::from(frame[6]);
    if len > SURFACE_TEXT_MAX_BYTES || frame.len() != HEADER_LEN + 3 + len {
        return None;
    }
    let text = core::str::from_utf8(&frame[7..7 + len]).ok()?;
    Some((frame[4], frame[5], text))
}

/// `OP_SURFACE_TEXT_FOCUS` field kinds (mirror imed wire semantics: password
/// fields get no preedit/candidates and never train personalization).
pub const SURFACE_FIELD_TEXT: u8 = 0;
pub const SURFACE_FIELD_PASSWORD: u8 = 1;

pub const SURFACE_TEXT_FOCUS_FRAME_LEN: usize = HEADER_LEN + 10;

/// Caret-anchor rect `(x, y, w, h)` in surface coordinates.
pub type CaretRect = (u16, u16, u16, u16);

/// Encodes a widget-focus announcement (caret rect in surface coordinates).
#[must_use]
pub fn encode_surface_text_focus(
    focused: bool,
    field_kind: u8,
    caret: CaretRect,
) -> [u8; SURFACE_TEXT_FOCUS_FRAME_LEN] {
    let mut f = [0u8; SURFACE_TEXT_FOCUS_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_TEXT_FOCUS));
    f[4] = u8::from(focused);
    f[5] = field_kind;
    f[6..8].copy_from_slice(&caret.0.to_le_bytes());
    f[8..10].copy_from_slice(&caret.1.to_le_bytes());
    f[10..12].copy_from_slice(&caret.2.to_le_bytes());
    f[12..14].copy_from_slice(&caret.3.to_le_bytes());
    f
}

/// `(focused, field_kind, caret x/y/w/h)`.
#[must_use]
pub fn decode_surface_text_focus(frame: &[u8]) -> Option<(bool, u8, CaretRect)> {
    if !has_op(frame, OP_SURFACE_TEXT_FOCUS) || frame.len() != SURFACE_TEXT_FOCUS_FRAME_LEN {
        return None;
    }
    let x = u16::from_le_bytes([frame[6], frame[7]]);
    let y = u16::from_le_bytes([frame[8], frame[9]]);
    let w = u16::from_le_bytes([frame[10], frame[11]]);
    let h = u16::from_le_bytes([frame[12], frame[13]]);
    Some((frame[4] != 0, frame[5], (x, y, w, h)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_surface::OP_SURFACE_INPUT;

    #[test]
    fn surface_text_round_trip_and_bounds() {
        let (f, n) = encode_surface_text(SURFACE_TEXT_COMMIT, 0, "éâ").unwrap();
        assert_eq!(&f[..HEADER_LEN], &header(OP_SURFACE_TEXT));
        assert_eq!(decode_surface_text(&f[..n]), Some((SURFACE_TEXT_COMMIT, 0, "éâ")));
        // Empty payload is valid (preedit clear); oversize rejects on encode.
        let (f, n) = encode_surface_text(SURFACE_TEXT_PREEDIT, 2, "").unwrap();
        assert_eq!(decode_surface_text(&f[..n]), Some((SURFACE_TEXT_PREEDIT, 2, "")));
        let long = [b'x'; SURFACE_TEXT_MAX_BYTES + 1];
        let long = core::str::from_utf8(&long).unwrap();
        assert!(encode_surface_text(SURFACE_TEXT_COMMIT, 0, long).is_none());
    }

    #[test]
    fn test_reject_surface_text_malformed() {
        let (f, n) = encode_surface_text(SURFACE_TEXT_COMMIT, 0, "abc").unwrap();
        // Truncation, length-field lies, invalid UTF-8 all fail closed.
        assert_eq!(decode_surface_text(&f[..n - 1]), None);
        let mut lying = f;
        lying[6] = 60;
        assert_eq!(decode_surface_text(&lying[..n]), None);
        let mut bad_utf8 = f;
        bad_utf8[7] = 0xFF;
        assert_eq!(decode_surface_text(&bad_utf8[..n]), None);
        let mut wrong_op = f;
        wrong_op[3] = OP_SURFACE_INPUT;
        assert_eq!(decode_surface_text(&wrong_op[..n]), None);
    }

    #[test]
    fn surface_text_focus_round_trip() {
        let f = encode_surface_text_focus(true, SURFACE_FIELD_PASSWORD, (10, 20, 2, 18));
        assert_eq!(
            decode_surface_text_focus(&f),
            Some((true, SURFACE_FIELD_PASSWORD, (10, 20, 2, 18)))
        );
        assert_eq!(decode_surface_text_focus(&f[..f.len() - 1]), None);
        let clear = encode_surface_text_focus(false, SURFACE_FIELD_TEXT, (0, 0, 0, 0));
        assert_eq!(
            decode_surface_text_focus(&clear),
            Some((false, SURFACE_FIELD_TEXT, (0, 0, 0, 0)))
        );
    }
}
