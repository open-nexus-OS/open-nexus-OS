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
/// widget focus and CLAIMS ITS OWN surface id (windowd's server endpoint
/// carries no per-sender identity for app processes today — same trust level
/// and same recorded follow-up as `OP_SURFACE_CONTROL`: enforce the sender
/// once the execd requester-id pattern lands). Payload: `surface_id:u32,
/// focused:u8, field_kind:u8, caret x/y/w/h:u16×4` (surface coordinates —
/// the caret rect anchors the OSK / candidate strip).
pub const OP_SURFACE_TEXT_FOCUS: u8 = 22;
/// windowd → app: region push (RFC-0076/0077) — locale tag, timezone and
/// hour format, sent at event-channel attach and on every settings change
/// (windowd watches settingsd). Apps must accept it only on windowd's
/// established push channel. Payload: `hour_fmt:u8 (0=24h, 1=12h),
/// locale_len:u8, locale[..16], tz_len:u8, tz[..32]`.
pub const OP_SURFACE_REGION: u8 = 23;

/// `OP_SURFACE_REGION` hour-format values.
pub const REGION_HOUR_24: u8 = 0;
pub const REGION_HOUR_12: u8 = 1;

/// Bounds (RFC-0077/0076): BCP-47-ish tag / IANA zone name.
pub const REGION_LOCALE_MAX: usize = 16;
pub const REGION_TZ_MAX: usize = 32;
pub const SURFACE_REGION_FRAME_MAX: usize = HEADER_LEN + 3 + REGION_LOCALE_MAX + REGION_TZ_MAX;

/// Encodes a region push; `None` when a field exceeds its bound.
#[must_use]
pub fn encode_surface_region(
    hour_fmt: u8,
    locale: &str,
    tz: &str,
) -> Option<([u8; SURFACE_REGION_FRAME_MAX], usize)> {
    let (l, t) = (locale.as_bytes(), tz.as_bytes());
    if l.len() > REGION_LOCALE_MAX || t.len() > REGION_TZ_MAX {
        return None;
    }
    let mut f = [0u8; SURFACE_REGION_FRAME_MAX];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_REGION));
    f[4] = hour_fmt;
    f[5] = l.len() as u8;
    let mut n = 6;
    f[n..n + l.len()].copy_from_slice(l);
    n += l.len();
    f[n] = t.len() as u8;
    n += 1;
    f[n..n + t.len()].copy_from_slice(t);
    n += t.len();
    Some((f, n))
}

/// `(hour_fmt, locale, tz)` — fail-closed on truncation/oversize/UTF-8.
#[must_use]
pub fn decode_surface_region(frame: &[u8]) -> Option<(u8, &str, &str)> {
    if !has_op(frame, OP_SURFACE_REGION) || frame.len() < HEADER_LEN + 3 {
        return None;
    }
    let l_len = usize::from(frame[5]);
    if l_len > REGION_LOCALE_MAX || frame.len() < 6 + l_len + 1 {
        return None;
    }
    let locale = core::str::from_utf8(&frame[6..6 + l_len]).ok()?;
    let t_off = 6 + l_len;
    let t_len = usize::from(frame[t_off]);
    if t_len > REGION_TZ_MAX || frame.len() != t_off + 1 + t_len {
        return None;
    }
    let tz = core::str::from_utf8(&frame[t_off + 1..t_off + 1 + t_len]).ok()?;
    Some((frame[4], locale, tz))
}

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

pub const SURFACE_TEXT_FOCUS_FRAME_LEN: usize = HEADER_LEN + 14;

/// Caret-anchor rect `(x, y, w, h)` in surface coordinates.
pub type CaretRect = (u16, u16, u16, u16);

/// Encodes a widget-focus announcement (caret rect in surface coordinates).
#[must_use]
pub fn encode_surface_text_focus(
    surface_id: u32,
    focused: bool,
    field_kind: u8,
    caret: CaretRect,
) -> [u8; SURFACE_TEXT_FOCUS_FRAME_LEN] {
    let mut f = [0u8; SURFACE_TEXT_FOCUS_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_TEXT_FOCUS));
    f[4..8].copy_from_slice(&surface_id.to_le_bytes());
    f[8] = u8::from(focused);
    f[9] = field_kind;
    f[10..12].copy_from_slice(&caret.0.to_le_bytes());
    f[12..14].copy_from_slice(&caret.1.to_le_bytes());
    f[14..16].copy_from_slice(&caret.2.to_le_bytes());
    f[16..18].copy_from_slice(&caret.3.to_le_bytes());
    f
}

/// `(surface_id, focused, field_kind, caret x/y/w/h)`.
#[must_use]
pub fn decode_surface_text_focus(frame: &[u8]) -> Option<(u32, bool, u8, CaretRect)> {
    if !has_op(frame, OP_SURFACE_TEXT_FOCUS) || frame.len() != SURFACE_TEXT_FOCUS_FRAME_LEN {
        return None;
    }
    let surface_id = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let x = u16::from_le_bytes([frame[10], frame[11]]);
    let y = u16::from_le_bytes([frame[12], frame[13]]);
    let w = u16::from_le_bytes([frame[14], frame[15]]);
    let h = u16::from_le_bytes([frame[16], frame[17]]);
    Some((surface_id, frame[8] != 0, frame[9], (x, y, w, h)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_surface::OP_SURFACE_INPUT;

    #[test]
    fn surface_region_round_trip_and_bounds() {
        let (f, n) = encode_surface_region(REGION_HOUR_24, "de-DE", "Europe/Berlin").unwrap();
        assert_eq!(
            decode_surface_region(&f[..n]),
            Some((REGION_HOUR_24, "de-DE", "Europe/Berlin"))
        );
        // Empty locale is valid (unset); oversize rejects on encode.
        let (f, n) = encode_surface_region(REGION_HOUR_12, "", "UTC").unwrap();
        assert_eq!(decode_surface_region(&f[..n]), Some((REGION_HOUR_12, "", "UTC")));
        assert!(encode_surface_region(0, "x-way-too-long-locale", "UTC").is_none());
        // Reject paths: truncation + lying length fields.
        let (f, n) = encode_surface_region(0, "de", "UTC").unwrap();
        assert_eq!(decode_surface_region(&f[..n - 1]), None);
        let mut lying = f;
        lying[5] = 12;
        assert_eq!(decode_surface_region(&lying[..n]), None);
    }

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
        let f = encode_surface_text_focus(7, true, SURFACE_FIELD_PASSWORD, (10, 20, 2, 18));
        assert_eq!(
            decode_surface_text_focus(&f),
            Some((7, true, SURFACE_FIELD_PASSWORD, (10, 20, 2, 18)))
        );
        assert_eq!(decode_surface_text_focus(&f[..f.len() - 1]), None);
        let clear = encode_surface_text_focus(1, false, SURFACE_FIELD_TEXT, (0, 0, 0, 0));
        assert_eq!(
            decode_surface_text_focus(&clear),
            Some((1, false, SURFACE_FIELD_TEXT, (0, 0, 0, 0)))
        );
    }
}
