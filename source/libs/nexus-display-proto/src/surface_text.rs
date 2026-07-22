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

/// Bounds (RFC-0077/0076): BCP-47-ish tag / IANA zone name / keymap tag.
pub const REGION_LOCALE_MAX: usize = 16;
pub const REGION_TZ_MAX: usize = 32;
pub const REGION_KEYMAP_MAX: usize = 8;
pub const SURFACE_REGION_FRAME_MAX: usize =
    HEADER_LEN + 4 + REGION_LOCALE_MAX + REGION_TZ_MAX + REGION_KEYMAP_MAX;

/// Encodes a region push; `None` when a field exceeds its bound.
#[must_use]
pub fn encode_surface_region(
    hour_fmt: u8,
    locale: &str,
    tz: &str,
    keymap: &str,
) -> Option<([u8; SURFACE_REGION_FRAME_MAX], usize)> {
    let (l, t, k) = (locale.as_bytes(), tz.as_bytes(), keymap.as_bytes());
    if l.len() > REGION_LOCALE_MAX || t.len() > REGION_TZ_MAX || k.len() > REGION_KEYMAP_MAX {
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
    // Keymap tag (RFC-0075 Phase 8b) — a TRAILING field: decoders accept
    // its absence (older encoders) as an empty tag.
    f[n] = k.len() as u8;
    n += 1;
    f[n..n + k.len()].copy_from_slice(k);
    n += k.len();
    Some((f, n))
}

/// `(hour_fmt, locale, tz, keymap)` — fail-closed on truncation/oversize/
/// UTF-8. The keymap tail is OPTIONAL (absent = empty, pre-8b frames).
#[must_use]
pub fn decode_surface_region(frame: &[u8]) -> Option<(u8, &str, &str, &str)> {
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
    if t_len > REGION_TZ_MAX || frame.len() < t_off + 1 + t_len {
        return None;
    }
    let tz = core::str::from_utf8(&frame[t_off + 1..t_off + 1 + t_len]).ok()?;
    let k_off = t_off + 1 + t_len;
    let keymap = if frame.len() == k_off {
        "" // pre-8b frame — no keymap tail
    } else {
        let k_len = usize::from(*frame.get(k_off)?);
        if k_len > REGION_KEYMAP_MAX || frame.len() != k_off + 1 + k_len {
            return None;
        }
        core::str::from_utf8(&frame[k_off + 1..k_off + 1 + k_len]).ok()?
    };
    Some((frame[4], locale, tz, keymap))
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

/// windowd → the ime-ui overlay (RFC-0075 Phase 3): the composition strip
/// state — preedit preview + one bounded candidate page. Two kinds ride one
/// op so a single push atomically replaces the strip's line.
pub const OP_SURFACE_IME_STATE: u8 = 24;

/// `OP_SURFACE_IME_STATE` kind: preedit text (empty clears the line).
pub const IME_STATE_PREEDIT: u8 = 0;
/// `OP_SURFACE_IME_STATE` kind: candidate page (count == 0 clears it).
pub const IME_STATE_CANDIDATES: u8 = 1;

/// Preedit bound (mirrors the imed wire bound).
pub const IME_PREEDIT_MAX: usize = 64;
/// Candidates per page (mirrors the imed wire bound).
pub const IME_CANDIDATES_MAX: usize = 8;
/// Bytes per candidate (mirrors the imed wire bound).
pub const IME_CANDIDATE_MAX_BYTES: usize = 32;
/// Maximum `OP_SURFACE_IME_STATE` frame:
/// header + kind + page + count + 8 × (len + 32).
pub const SURFACE_IME_STATE_FRAME_MAX: usize =
    HEADER_LEN + 3 + IME_CANDIDATES_MAX * (1 + IME_CANDIDATE_MAX_BYTES);

/// Preedit push: `[hdr, kind=PREEDIT, len:u8, text…]`.
#[must_use]
pub fn encode_ime_preedit(text: &str) -> Option<([u8; SURFACE_IME_STATE_FRAME_MAX], usize)> {
    let b = text.as_bytes();
    if b.len() > IME_PREEDIT_MAX {
        return None;
    }
    let mut f = [0u8; SURFACE_IME_STATE_FRAME_MAX];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_IME_STATE));
    f[4] = IME_STATE_PREEDIT;
    f[5] = b.len() as u8;
    f[6..6 + b.len()].copy_from_slice(b);
    Some((f, 6 + b.len()))
}

/// Candidate-page push: `[hdr, kind=CANDIDATES, page:u8, count:u8,
/// (len:u8, bytes…)×count]`.
#[must_use]
pub fn encode_ime_candidates(
    page: u8,
    candidates: &[&str],
) -> Option<([u8; SURFACE_IME_STATE_FRAME_MAX], usize)> {
    if candidates.len() > IME_CANDIDATES_MAX {
        return None;
    }
    let mut f = [0u8; SURFACE_IME_STATE_FRAME_MAX];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_IME_STATE));
    f[4] = IME_STATE_CANDIDATES;
    f[5] = page;
    f[6] = candidates.len() as u8;
    let mut n = 7;
    for c in candidates {
        let b = c.as_bytes();
        if b.is_empty() || b.len() > IME_CANDIDATE_MAX_BYTES {
            return None;
        }
        f[n] = b.len() as u8;
        n += 1;
        f[n..n + b.len()].copy_from_slice(b);
        n += b.len();
    }
    Some((f, n))
}

/// The decoded strip push (borrowed slices; fail-closed).
pub enum ImeStatePush<'a> {
    Preedit(&'a str),
    /// `(page, candidates ≤ 8)` — unused slots are empty strings.
    Candidates(u8, [&'a str; IME_CANDIDATES_MAX], usize),
}

/// Fail-closed decode of `OP_SURFACE_IME_STATE`.
#[must_use]
pub fn decode_ime_state(frame: &[u8]) -> Option<ImeStatePush<'_>> {
    if !has_op(frame, OP_SURFACE_IME_STATE) || frame.len() < HEADER_LEN + 1 {
        return None;
    }
    match frame[4] {
        IME_STATE_PREEDIT => {
            let len = usize::from(*frame.get(5)?);
            if len > IME_PREEDIT_MAX || frame.len() != 6 + len {
                return None;
            }
            core::str::from_utf8(&frame[6..6 + len]).ok().map(ImeStatePush::Preedit)
        }
        IME_STATE_CANDIDATES => {
            let page = *frame.get(5)?;
            let count = usize::from(*frame.get(6)?);
            if count > IME_CANDIDATES_MAX {
                return None;
            }
            let mut out: [&str; IME_CANDIDATES_MAX] = [""; IME_CANDIDATES_MAX];
            let mut n = 7;
            for slot in out.iter_mut().take(count) {
                let len = usize::from(*frame.get(n)?);
                if len == 0 || len > IME_CANDIDATE_MAX_BYTES {
                    return None;
                }
                n += 1;
                *slot = core::str::from_utf8(frame.get(n..n + len)?).ok()?;
                n += len;
            }
            if frame.len() != n {
                return None;
            }
            Some(ImeStatePush::Candidates(page, out, count))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_surface::OP_SURFACE_INPUT;

    #[test]
    fn surface_region_round_trip_and_bounds() {
        let (f, n) = encode_surface_region(REGION_HOUR_24, "de-DE", "Europe/Berlin", "de").unwrap();
        assert_eq!(
            decode_surface_region(&f[..n]),
            Some((REGION_HOUR_24, "de-DE", "Europe/Berlin", "de"))
        );
        // Empty locale/keymap are valid (unset); oversize rejects on encode.
        let (f, n) = encode_surface_region(REGION_HOUR_12, "", "UTC", "").unwrap();
        assert_eq!(decode_surface_region(&f[..n]), Some((REGION_HOUR_12, "", "UTC", "")));
        assert!(encode_surface_region(0, "x-way-too-long-locale", "UTC", "").is_none());
        assert!(encode_surface_region(0, "de", "UTC", "way-too-long").is_none());
        // A pre-8b frame WITHOUT the keymap tail decodes with an empty tag.
        let (f, n) = encode_surface_region(0, "de", "UTC", "").unwrap();
        assert_eq!(decode_surface_region(&f[..n - 1]), Some((0, "de", "UTC", "")));
        // Reject paths: truncation into the tz field + lying length fields.
        assert_eq!(decode_surface_region(&f[..n - 2]), None);
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

    #[test]
    fn ime_state_round_trips_and_rejects() {
        let (f, n) = encode_ime_preedit("にほ").unwrap();
        match decode_ime_state(&f[..n]) {
            Some(ImeStatePush::Preedit(t)) => assert_eq!(t, "にほ"),
            _ => panic!("preedit decodes"),
        }
        let (f, n) = encode_ime_candidates(1, &["你好", "泥"]).unwrap();
        match decode_ime_state(&f[..n]) {
            Some(ImeStatePush::Candidates(page, items, count)) => {
                assert_eq!((page, count), (1, 2));
                assert_eq!(items[0], "你好");
                assert_eq!(items[1], "泥");
            }
            _ => panic!("candidates decode"),
        }
        // Truncation + oversize fail closed.
        assert!(decode_ime_state(&f[..n - 1]).is_none());
        assert!(encode_ime_candidates(0, &[""]).is_none());
        let long = core::str::from_utf8(&[b'x'; IME_CANDIDATE_MAX_BYTES + 1]).unwrap();
        assert!(encode_ime_candidates(0, &[long]).is_none());
    }
}
