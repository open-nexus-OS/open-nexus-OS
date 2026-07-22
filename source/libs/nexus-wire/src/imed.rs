// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! imed v1 wire protocol (RFC-0075): text focus, key delivery and composition
//! output for the IME authority. inputd forwards resolved keys (`OP_KEY`,
//! source=hw), windowd relays focus (`OP_SET_FOCUS`) and candidate selection
//! (`OP_CANDIDATE_SELECT`); imed pushes `OP_COMMIT`/`OP_PREEDIT`/
//! `OP_CANDIDATES` back to windowd for routing to the focused surface.
//! Identity is enforced by the server via kernel `sender_service_id` — the
//! wire carries no caller identity by design.

/// First magic byte (`'I'`).
pub const MAGIC0: u8 = b'I';
/// Second magic byte (`'E'`).
pub const MAGIC1: u8 = b'E';
/// Protocol version.
pub const VERSION: u8 = 1;

/// Focus transition relayed by windowd (aggregated surface + widget focus).
pub const OP_SET_FOCUS: u8 = 1;
/// One resolved key (inputd hardware path or vetted OSK injection).
pub const OP_KEY: u8 = 2;
/// Committed text push (imed → windowd → focused surface).
pub const OP_COMMIT: u8 = 3;
/// Preedit snapshot push (composition in progress; empty text clears).
pub const OP_PREEDIT: u8 = 4;
/// Candidate page push (CJK engines, RFC-0075 Phase 3).
pub const OP_CANDIDATES: u8 = 5;
/// Candidate pick relayed by windowd (index into the current page).
pub const OP_CANDIDATE_SELECT: u8 = 6;
/// Editing-action push (imed → windowd): an action that passed through
/// composition (Enter/Backspace/Tab/Escape) for the focused surface —
/// windowd translates it to `SURFACE_TEXT_ACTION` on the app channel.
pub const OP_ACTION: u8 = 7;
/// Engine-layout switch (RFC-0075 Phase 3): inputd forwards `input.keymap`
/// changes on the MAIN endpoint (identity-gated); the OSK's globe key sends
/// it on the DEDICATED osk endpoint (capability-gated). The tag selects the
/// composition engine (`us`/`de` → Latin, `jp`, `kr`, `zh`).
pub const OP_SET_LAYOUT: u8 = 8;

/// Maximum layout-tag bytes (`OP_SET_LAYOUT`).
pub const LAYOUT_MAX_BYTES: usize = 8;

/// Operation succeeded.
pub const STATUS_OK: u8 = 0;
/// Request frame was malformed.
pub const STATUS_MALFORMED: u8 = 1;
/// Sender identity is not allowed to perform this op.
pub const STATUS_DENIED: u8 = 2;
/// Op understood but not applicable in the current state.
pub const STATUS_UNSUPPORTED: u8 = 3;

/// `OP_KEY` source: hardware chain (inputd).
pub const KEY_SOURCE_HW: u8 = 0;
/// `OP_KEY` source: on-screen keyboard (ime-ui via app-host, policyd-gated).
pub const KEY_SOURCE_OSK: u8 = 1;

/// `OP_KEY` kind: printable text scalar in `ch`.
pub const KEY_KIND_TEXT: u8 = 0;
/// `OP_KEY` kind: dead key (accent scalar in `ch`).
pub const KEY_KIND_DEAD: u8 = 1;
/// `OP_KEY` kind: editing action in `action` (`ch` is 0).
pub const KEY_KIND_ACTION: u8 = 2;

/// Editing actions carried by `OP_KEY` (`KEY_KIND_ACTION`).
pub const ACTION_ENTER: u8 = 0;
/// Escape (cancels composition; passes through otherwise).
pub const ACTION_ESCAPE: u8 = 1;
/// Backspace.
pub const ACTION_BACKSPACE: u8 = 2;
/// Tab.
pub const ACTION_TAB: u8 = 3;

/// `OP_SET_FOCUS` field kind: plain text field.
pub const FIELD_KIND_TEXT: u8 = 0;
/// `OP_SET_FOCUS` field kind: password field — no preedit push, no
/// candidates, no learning (RFC-0075 security invariant, enforced in imed).
pub const FIELD_KIND_PASSWORD: u8 = 1;

/// Maximum committed/preedit text bytes per frame (RFC-0075 bound).
pub const TEXT_MAX_BYTES: usize = 64;
/// Maximum candidates per page (RFC-0075 bound).
pub const CANDIDATES_MAX: usize = 8;
/// Maximum bytes per candidate string (RFC-0075 bound).
pub const CANDIDATE_MAX_BYTES: usize = 32;
/// Maximum packed candidate-list payload (`len:u8 + bytes` per entry).
pub const CANDIDATE_LIST_MAX_BYTES: usize = CANDIDATES_MAX * (1 + CANDIDATE_MAX_BYTES);

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// Focus: `[I, E, ver, OP_SET_FOCUS, surface_id:u64, focused:u8,
    /// field_kind:u8, caret x/y/w/h:u16×4]` (caret rect anchors OSK/candidates).
    request fixed encode_set_focus / decode_set_focus (op = OP_SET_FOCUS) {
        surface_id: u64le,
        focused: u8,
        field_kind: u8,
        caret_x: u16le,
        caret_y: u16le,
        caret_w: u16le,
        caret_h: u16le,
    }
    /// Key: `[I, E, ver, OP_KEY, source:u8, kind:u8, ch:u32, action:u8, modifiers:u8]`.
    request fixed encode_key / decode_key (op = OP_KEY) {
        source: u8,
        kind: u8,
        ch: u32le,
        action: u8,
        modifiers: u8,
    }
    /// Commit push: `[I, E, ver, OP_COMMIT, surface_id:u64, text_len:u8, text...]`.
    request encode_commit / decode_commit (op = OP_COMMIT) {
        surface_id: u64le,
        text: str8(min = 1, max = TEXT_MAX_BYTES),
    }
    /// Preedit push: `[I, E, ver, OP_PREEDIT, surface_id:u64, caret:u8,
    /// text_len:u8, text...]` (empty text clears the preedit).
    request encode_preedit / decode_preedit (op = OP_PREEDIT) {
        surface_id: u64le,
        caret: u8,
        text: str8(min = 0, max = TEXT_MAX_BYTES),
    }
    /// Candidates push: `[I, E, ver, OP_CANDIDATES, surface_id:u64, page:u8,
    /// count:u8, list_len:u16, packed...]` — packed entries `len:u8, bytes...`
    /// (see `encode_candidate_list`/`candidate_at`).
    request encode_candidates / decode_candidates (op = OP_CANDIDATES) {
        surface_id: u64le,
        page: u8,
        count: u8,
        list: bytes16(min = 0, max = CANDIDATE_LIST_MAX_BYTES),
    }
    /// Candidate pick: `[I, E, ver, OP_CANDIDATE_SELECT, index:u8]`.
    request fixed encode_candidate_select / decode_candidate_select (op = OP_CANDIDATE_SELECT) {
        index: u8,
    }
    /// Layout switch: `[I, E, ver, OP_SET_LAYOUT, tag_len:u8, tag...]`.
    request encode_set_layout / decode_set_layout (op = OP_SET_LAYOUT) {
        layout: str8(min = 1, max = LAYOUT_MAX_BYTES),
    }
    /// Action push: `[I, E, ver, OP_ACTION, surface_id:u64, action:u8]`
    /// (`ACTION_*` code that passed through composition).
    request fixed encode_action / decode_action (op = OP_ACTION) {
        surface_id: u64le,
        action: u8,
    }
    /// Response: `[I, E, ver, op|0x80, status:u8]`.
    reply fixed encode_response / decode_response (op = caller) {
        status: u8,
    }
    /// OSK-endpoint reply (reply-cap probes only): `[I, E, ver, op|0x80,
    /// status:u8, text_len:u8, text...]` — `text` echoes the COMMIT this
    /// step produced back to the INJECTING sender itself (never a third
    /// party; the ime-ui app sends fire-and-forget and gets no reply).
    reply encode_osk_reply / decode_osk_reply (op = caller) {
        status: u8,
        text: str8(min = 0, max = TEXT_MAX_BYTES),
    }
}

/// Packs candidate strings into the `OP_CANDIDATES` list payload.
/// Fails (`None`) beyond the RFC bounds (count, per-candidate size, empty).
pub fn encode_candidate_list(candidates: &[&str], out: &mut [u8]) -> Option<usize> {
    if candidates.len() > CANDIDATES_MAX {
        return None;
    }
    let mut used = 0usize;
    for cand in candidates {
        let bytes = cand.as_bytes();
        if bytes.is_empty() || bytes.len() > CANDIDATE_MAX_BYTES {
            return None;
        }
        let end = used.checked_add(1 + bytes.len())?;
        if end > out.len() || end > CANDIDATE_LIST_MAX_BYTES {
            return None;
        }
        out[used] = bytes.len() as u8;
        out[used + 1..end].copy_from_slice(bytes);
        used = end;
    }
    Some(used)
}

/// Reads candidate `index` from a packed list payload (fail-closed on any
/// malformed entry: zero/oversized length, truncation, invalid UTF-8).
pub fn candidate_at(list: &[u8], index: u8) -> Option<&str> {
    let mut rest = list;
    let mut seen = 0u8;
    while !rest.is_empty() {
        let len = usize::from(*rest.first()?);
        if len == 0 || len > CANDIDATE_MAX_BYTES || rest.len() < 1 + len {
            return None;
        }
        let (entry, tail) = rest[1..].split_at(len);
        if seen == index {
            return core::str::from_utf8(entry).ok();
        }
        seen = seen.checked_add(1)?;
        if seen > CANDIDATES_MAX as u8 {
            return None;
        }
        rest = tail;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_focus_golden_bytes_and_roundtrip() {
        let frame = encode_set_focus(7, 1, FIELD_KIND_PASSWORD, 10, 20, 2, 18);
        assert_eq!(&frame[..4], &[b'I', b'E', 1, OP_SET_FOCUS]);
        assert_eq!(&frame[4..12], &7u64.to_le_bytes());
        assert_eq!(frame[12], 1);
        assert_eq!(frame[13], FIELD_KIND_PASSWORD);
        assert_eq!(&frame[14..16], &10u16.to_le_bytes());
        assert_eq!(decode_set_focus(&frame), Some((7, 1, FIELD_KIND_PASSWORD, 10, 20, 2, 18)));
    }

    #[test]
    fn key_golden_bytes_and_roundtrip() {
        let frame = encode_key(KEY_SOURCE_HW, KEY_KIND_TEXT, 'ä' as u32, 0, 0b10);
        assert_eq!(&frame[..4], &[b'I', b'E', 1, OP_KEY]);
        assert_eq!(&frame[6..10], &(0xE4u32).to_le_bytes());
        assert_eq!(decode_key(&frame), Some((KEY_SOURCE_HW, KEY_KIND_TEXT, 0xE4, 0, 0b10)));
    }

    #[test]
    fn commit_and_preedit_roundtrip() {
        let mut buf = [0u8; 96];
        let n = encode_commit(9, "é", &mut buf).unwrap();
        assert_eq!(decode_commit(&buf[..n]), Some((9, "é")));
        // Empty commit rejects (min = 1); empty preedit clears (min = 0).
        assert_eq!(encode_commit(9, "", &mut buf), None);
        let n = encode_preedit(9, 3, "", &mut buf).unwrap();
        assert_eq!(decode_preedit(&buf[..n]), Some((9, 3, "")));
    }

    #[test]
    fn candidates_pack_and_lookup() {
        let mut list = [0u8; CANDIDATE_LIST_MAX_BYTES];
        let used = encode_candidate_list(&["日本語", "にほんご"], &mut list).unwrap();
        let mut frame = [0u8; 512];
        let n = encode_candidates(4, 0, 2, &list[..used], &mut frame).unwrap();
        let (surface, page, count, packed) = decode_candidates(&frame[..n]).unwrap();
        assert_eq!((surface, page, count), (4, 0, 2));
        assert_eq!(candidate_at(packed, 0), Some("日本語"));
        assert_eq!(candidate_at(packed, 1), Some("にほんご"));
        assert_eq!(candidate_at(packed, 2), None);
    }

    #[test]
    fn test_reject_candidate_list_bounds() {
        let mut list = [0u8; 512];
        // Too many candidates.
        let nine: [&str; 9] = ["a"; 9];
        assert_eq!(encode_candidate_list(&nine, &mut list), None);
        // Oversized single candidate (> 32 bytes).
        let long = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        assert_eq!(encode_candidate_list(&[long], &mut list), None);
        // Empty candidate.
        assert_eq!(encode_candidate_list(&[""], &mut list), None);
        // Malformed packed payloads fail closed on lookup.
        assert_eq!(candidate_at(&[0], 0), None); // zero length
        assert_eq!(candidate_at(&[5, b'a'], 0), None); // truncated entry
        assert_eq!(candidate_at(&[33], 0), None); // oversized length
    }

    #[test]
    fn response_roundtrip() {
        let frame = encode_response(OP_KEY, STATUS_OK);
        assert_eq!(decode_response(OP_KEY, &frame), Some(STATUS_OK));
        assert_eq!(decode_response(OP_SET_FOCUS, &frame), None);
    }

    #[test]
    fn reject_truncation_and_mutation_matrix() {
        let focus = encode_set_focus(7, 1, 0, 1, 2, 3, 4);
        crate::codec::testing::assert_reject_matrix(&focus, 4, &|f| decode_set_focus(f).is_some());
        let key = encode_key(KEY_SOURCE_OSK, KEY_KIND_ACTION, 0, ACTION_ENTER, 0);
        crate::codec::testing::assert_reject_matrix(&key, 4, &|f| decode_key(f).is_some());
        let mut buf = [0u8; 96];
        let n = encode_commit(1, "abc", &mut buf).unwrap();
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| decode_commit(f).is_some());
        let sel = encode_candidate_select(3);
        crate::codec::testing::assert_reject_matrix(&sel, 4, &|f| {
            decode_candidate_select(f).is_some()
        });
        let act = encode_action(7, ACTION_BACKSPACE);
        assert_eq!(decode_action(&act), Some((7, ACTION_BACKSPACE)));
        crate::codec::testing::assert_reject_matrix(&act, 4, &|f| decode_action(f).is_some());
        let rsp = encode_response(OP_KEY, STATUS_OK);
        crate::codec::testing::assert_reject_matrix(&rsp, 4, &|f| {
            decode_response(OP_KEY, f).is_some()
        });
    }

    #[test]
    fn set_layout_round_trip_and_rejects() {
        let mut buf = [0u8; 16];
        let n = encode_set_layout("jp", &mut buf).expect("encodes");
        assert_eq!(decode_set_layout(&buf[..n]), Some("jp"));
        // Empty and oversized tags fail closed.
        assert_eq!(encode_set_layout("", &mut buf), None);
        assert_eq!(encode_set_layout("wayyytoolong", &mut buf), None);
        // Truncation rejects.
        assert_eq!(decode_set_layout(&buf[..n - 1]), None);
    }

    #[test]
    fn osk_reply_round_trip_and_rejects() {
        let mut buf = [0u8; 80];
        let n = encode_osk_reply(OP_KEY, STATUS_OK, "ん", &mut buf).expect("encodes");
        assert_eq!(decode_osk_reply(OP_KEY, &buf[..n]), Some((STATUS_OK, "ん")));
        // Empty echo is legal (no commit this step).
        let n = encode_osk_reply(OP_KEY, STATUS_OK, "", &mut buf).expect("encodes");
        assert_eq!(decode_osk_reply(OP_KEY, &buf[..n]), Some((STATUS_OK, "")));
        // Wrong op and truncation reject.
        assert_eq!(decode_osk_reply(OP_SET_FOCUS, &buf[..n]), None);
        assert_eq!(decode_osk_reply(OP_KEY, &buf[..n - 1]), None);
    }
}
