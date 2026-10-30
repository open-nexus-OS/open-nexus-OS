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

/// windowd → app: text delivery to the FOCUSED surface (RFC-0075, RFC-0113). A layout source's
/// text comes from inputd through windowd, a composition source's through imed and windowd —
/// apps accept text only on this established push channel, never from peers. Payload:
/// `kind:u8, aux:u8, len:u8, seq:u32, t_ns:u64, text[..64]` (UTF-8, bounded); `seq`/`t_ns` are
/// the source key's (RFC-0113 §1) — the field measures key → present from them — and zero for
/// text no key produced (an on-screen-keyboard card).
pub const OP_SURFACE_TEXT: u8 = 21;
/// app → windowd: widget text-focus announcement (RFC-0075). The app owns
/// widget focus and CLAIMS ITS OWN surface id (windowd's server endpoint
/// carries no per-sender identity for app processes today — same trust level
/// and same recorded follow-up as `OP_SURFACE_CONTROL`: enforce the sender
/// once the execd requester-id pattern lands). Payload: `surface_id:u32,
/// focused:u8, field_kind:u8, caret x/y/w/h:u16×4` (surface coordinates —
/// the caret rect anchors the OSK / candidate strip).
pub const OP_SURFACE_TEXT_FOCUS: u8 = 22;
/// RETIRED (RFC-0083 P5, number reserved forever — never reuse): the
/// standalone region push (locale/tz/hour/keymap), folded into the
/// presentation snapshot (`surface_settings::OP_SURFACE_SETTINGS = 26`).
pub const OP_SURFACE_REGION: u8 = 23;

/// Hour-format values (the snapshot's `hour_fmt` field).
pub const REGION_HOUR_24: u8 = 0;
pub const REGION_HOUR_12: u8 = 1;

/// Bounds (RFC-0077/0076): BCP-47-ish tag / IANA zone name / keymap tag —
/// shared with the presentation snapshot's string fields.
pub const REGION_LOCALE_MAX: usize = 16;
pub const REGION_TZ_MAX: usize = 32;
pub const REGION_KEYMAP_MAX: usize = 8;

/// `OP_SURFACE_TEXT` kind: committed text in the payload.
pub const SURFACE_TEXT_COMMIT: u8 = 0;
/// `OP_SURFACE_TEXT` kind: preedit snapshot (aux = caret index; empty clears).
pub const SURFACE_TEXT_PREEDIT: u8 = 1;
/// `OP_SURFACE_TEXT` kind: editing action passed through composition
/// (aux = imed wire `ACTION_*`; payload empty).
pub const SURFACE_TEXT_ACTION: u8 = 2;

/// Maximum UTF-8 payload bytes per `OP_SURFACE_TEXT` frame (RFC-0075 bound).
pub const SURFACE_TEXT_MAX_BYTES: usize = 64;
/// `kind, aux, len` and the source key's stamp (`seq:u32, t_ns:u64`).
const SURFACE_TEXT_HEAD_LEN: usize = 3 + 4 + 8;
pub const SURFACE_TEXT_FRAME_MAX: usize =
    HEADER_LEN + SURFACE_TEXT_HEAD_LEN + SURFACE_TEXT_MAX_BYTES;

/// The source key of a text push (RFC-0113 §1): its stream sequence and when the system first
/// saw it; [`TextStamp::NONE`] for text no key produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStamp {
    pub seq: u32,
    pub t_ns: u64,
}

impl TextStamp {
    /// Text no key produced (an on-screen-keyboard card, an insert).
    pub const NONE: Self = Self { seq: 0, t_ns: 0 };
}

/// Encodes a text push; `None` when `text` exceeds the bound.
#[must_use]
pub fn encode_surface_text(
    kind: u8,
    aux: u8,
    text: &str,
    stamp: TextStamp,
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
    f[7..11].copy_from_slice(&stamp.seq.to_le_bytes());
    f[11..19].copy_from_slice(&stamp.t_ns.to_le_bytes());
    let start = HEADER_LEN + SURFACE_TEXT_HEAD_LEN;
    f[start..start + bytes.len()].copy_from_slice(bytes);
    Some((f, start + bytes.len()))
}

/// `(kind, aux, text, stamp)` — fail-closed on truncation, oversize or invalid UTF-8.
#[must_use]
pub fn decode_surface_text(frame: &[u8]) -> Option<(u8, u8, &str, TextStamp)> {
    let start = HEADER_LEN + SURFACE_TEXT_HEAD_LEN;
    if !has_op(frame, OP_SURFACE_TEXT) || frame.len() < start {
        return None;
    }
    let len = usize::from(frame[6]);
    if len > SURFACE_TEXT_MAX_BYTES || frame.len() != start + len {
        return None;
    }
    let seq = u32::from_le_bytes([frame[7], frame[8], frame[9], frame[10]]);
    let t_ns = u64::from_le_bytes([
        frame[11], frame[12], frame[13], frame[14], frame[15], frame[16], frame[17], frame[18],
    ]);
    let text = core::str::from_utf8(&frame[start..start + len]).ok()?;
    Some((frame[4], frame[5], text, TextStamp { seq, t_ns }))
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

/// The largest frame windowd sends a client on its event channel — windowd's outbox stores it
/// and every client's receive buffer holds it (RFC-0113: an app-host buffer of 64 bytes cut
/// every text over 57 bytes and every candidate page).
pub const CLIENT_EVENT_FRAME_MAX: usize = SURFACE_IME_STATE_FRAME_MAX;
const _: () = assert!(SURFACE_TEXT_FRAME_MAX <= CLIENT_EVENT_FRAME_MAX);

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

/// App → windowd (pointer-cursor hint): the app owns hover semantics INSIDE
/// its surface (windowd hit-tests only window chrome/edges) — a hint arms the
/// named shape while the pointer stays over that surface's body, and the app
/// clears it with `CURSOR_HINT_DEFAULT` on hover-out. Semantic ids (not
/// windowd shape-cache slots) so the wire stays stable; unknown ids are
/// rejected by the decoder.
pub const OP_SURFACE_CURSOR_HINT: u8 = 25;

/// Cursor-hint shape: the platform default pointer.
pub const CURSOR_HINT_DEFAULT: u8 = 0;
/// Cursor-hint shape: the text I-beam.
pub const CURSOR_HINT_TEXT: u8 = 1;

pub const SURFACE_CURSOR_HINT_FRAME_LEN: usize = HEADER_LEN + 5;

/// Encodes a cursor hint for `surface_id`.
#[must_use]
pub fn encode_surface_cursor_hint(
    surface_id: u32,
    shape: u8,
) -> [u8; SURFACE_CURSOR_HINT_FRAME_LEN] {
    let mut f = [0u8; SURFACE_CURSOR_HINT_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CURSOR_HINT));
    f[4..8].copy_from_slice(&surface_id.to_le_bytes());
    f[8] = shape;
    f
}

/// `(surface_id, shape)` — fail-closed on length/op/unknown shape.
#[must_use]
pub fn decode_surface_cursor_hint(frame: &[u8]) -> Option<(u32, u8)> {
    if !has_op(frame, OP_SURFACE_CURSOR_HINT) || frame.len() != SURFACE_CURSOR_HINT_FRAME_LEN {
        return None;
    }
    let shape = frame[8];
    if shape > CURSOR_HINT_TEXT {
        return None;
    }
    Some((u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]), shape))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_surface::OP_SURFACE_INPUT;

    #[test]
    fn surface_cursor_hint_round_trip() {
        let f = encode_surface_cursor_hint(7, CURSOR_HINT_TEXT);
        assert_eq!(decode_surface_cursor_hint(&f), Some((7, CURSOR_HINT_TEXT)));
        let f = encode_surface_cursor_hint(0, CURSOR_HINT_DEFAULT);
        assert_eq!(decode_surface_cursor_hint(&f), Some((0, CURSOR_HINT_DEFAULT)));
    }

    #[test]
    fn test_reject_surface_cursor_hint_malformed() {
        let f = encode_surface_cursor_hint(7, CURSOR_HINT_TEXT);
        // Truncation, unknown shape id, wrong op all fail closed.
        assert_eq!(decode_surface_cursor_hint(&f[..f.len() - 1]), None);
        let mut bad_shape = f;
        bad_shape[8] = 9;
        assert_eq!(decode_surface_cursor_hint(&bad_shape), None);
        let mut wrong_op = f;
        wrong_op[3] = OP_SURFACE_INPUT;
        assert_eq!(decode_surface_cursor_hint(&wrong_op), None);
    }

    #[test]
    fn surface_text_round_trip_and_bounds() {
        let stamp = TextStamp { seq: 41, t_ns: 9_000_000 };
        let (f, n) = encode_surface_text(SURFACE_TEXT_COMMIT, 0, "éâ", stamp).unwrap();
        assert_eq!(&f[..HEADER_LEN], &header(OP_SURFACE_TEXT));
        assert_eq!(decode_surface_text(&f[..n]), Some((SURFACE_TEXT_COMMIT, 0, "éâ", stamp)));
        // Empty payload is valid (preedit clear); oversize rejects on encode.
        let (f, n) = encode_surface_text(SURFACE_TEXT_PREEDIT, 2, "", TextStamp::NONE).unwrap();
        assert_eq!(
            decode_surface_text(&f[..n]),
            Some((SURFACE_TEXT_PREEDIT, 2, "", TextStamp::NONE))
        );
        let long = [b'x'; SURFACE_TEXT_MAX_BYTES + 1];
        let long = core::str::from_utf8(&long).unwrap();
        assert!(encode_surface_text(SURFACE_TEXT_COMMIT, 0, long, stamp).is_none());
        // The largest frame fits every receiver sized by SURFACE_TEXT_FRAME_MAX (app-host).
        let full = [b'y'; SURFACE_TEXT_MAX_BYTES];
        let full = core::str::from_utf8(&full).unwrap();
        let (_, n) = encode_surface_text(SURFACE_TEXT_COMMIT, 0, full, stamp).unwrap();
        assert_eq!(n, SURFACE_TEXT_FRAME_MAX);
    }

    #[test]
    fn test_reject_surface_text_malformed() {
        let (f, n) = encode_surface_text(SURFACE_TEXT_COMMIT, 0, "abc", TextStamp::NONE).unwrap();
        // Truncation, length-field lies, invalid UTF-8 all fail closed.
        assert_eq!(decode_surface_text(&f[..n - 1]), None);
        let mut lying = f;
        lying[6] = 60;
        assert_eq!(decode_surface_text(&lying[..n]), None);
        let mut bad_utf8 = f;
        bad_utf8[HEADER_LEN + SURFACE_TEXT_HEAD_LEN] = 0xFF;
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
