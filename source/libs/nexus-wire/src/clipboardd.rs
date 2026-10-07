// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: clipboardd wire v1 (RFC-0094 "Content transfer v1", TASK-0067) —
//! the ONE clipboard authority's request/reply frames in the `'C','B'`
//! envelope. v1 is TEXT: one inline UTF-8 item per write (≤
//! [`TEXT_MAX_BYTES`]), a bounded newest-first history ([`HISTORY_MAX`]),
//! previews in the list reply (≤ [`PREVIEW_MAX_BYTES`] per entry, so the
//! whole history fits one reply), a search query on the list request (the
//! filter runs AT the service, like the launcher's), and the focus truth
//! windowd pushes (`OP_FOCUS`, fire-and-forget, retained latest-wins) that
//! gates every read. Flavors beyond text and the VMO path are TASK-0087's
//! (append-only ops). Cross-device sync is a later consumer of the same
//! history: every entry already names its origin device (`device`, 0 = this
//! one) and carries a monotonic `seq`.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable (v1; ops append-only)
//! TEST_COVERAGE: round-trips + reject matrix below
//! RFC: docs/rfcs/RFC-0094-content-transfer-v1-clipboardd.md

/// Envelope magic byte 0.
pub const MAGIC0: u8 = b'C';
/// Envelope magic byte 1.
pub const MAGIC1: u8 = b'B';
/// Wire version.
pub const VERSION: u8 = 1;

/// Write one text item; it becomes the newest. Reply: [`encode_seq_reply`].
pub const OP_WRITE: u8 = 1;
/// Read one item's full text (`seq` = 0: the newest — the paste). Reply:
/// [`encode_read_reply`].
pub const OP_READ: u8 = 2;
/// List the history newest-first with previews, filtered by a query (empty =
/// everything). Reply: [`encode_list_reply`].
pub const OP_LIST: u8 = 3;
/// Make an older entry the newest again (copy-back). Reply: the new `seq`.
pub const OP_RESTORE: u8 = 4;
/// Drop every item. Reply: [`encode_status`].
pub const OP_CLEAR: u8 = 5;
/// windowd → clipboardd, fire-and-forget, retained latest-wins: the owners a
/// read is allowed for — the focused window's, the desktop surface's (the
/// shell) and the IME overlay's (the on-screen keyboard). The REQUEST's
/// identity is the kernel sender id, never a payload byte; these ids are what
/// that sender id is compared against, and only windowd may set them.
pub const OP_FOCUS: u8 = 6;

/// Reply status: served.
pub const STATUS_OK: u8 = 0;
/// Reply status: the request did not decode or broke a bound.
pub const STATUS_MALFORMED: u8 = 1;
/// Reply status: the sender is not an owner windowd granted (`clipboard-focus`).
pub const STATUS_DENIED: u8 = 2;
/// Reply status: nothing to read (empty history or an unknown `seq`).
pub const STATUS_EMPTY: u8 = 3;
/// Reply status: an op this version does not serve.
pub const STATUS_UNSUPPORTED: u8 = 4;

/// Largest text item (v1, inline; the VMO path for larger items is TASK-0087's).
pub const TEXT_MAX_BYTES: usize = 248;
/// Largest preview in a list entry (a card shows no more).
pub const PREVIEW_MAX_BYTES: usize = 120;
/// Largest search query on a list request.
pub const QUERY_MAX_BYTES: usize = 64;
/// History depth: the ring keeps this many newest items.
pub const HISTORY_MAX: usize = 16;
/// Fixed head of one packed list entry: `seq:u64, writer_sid:u64, device:u64, len:u8`.
pub const LIST_ENTRY_HEAD: usize = 8 + 8 + 8 + 1;
/// One packed list entry at most.
pub const LIST_ENTRY_MAX_BYTES: usize = LIST_ENTRY_HEAD + PREVIEW_MAX_BYTES;
/// The packed list payload bound (`count ≤ HISTORY_MAX`).
pub const LIST_MAX_BYTES: usize = HISTORY_MAX * LIST_ENTRY_MAX_BYTES;
/// A buffer that holds any reply of this protocol.
pub const REPLY_MAX_BYTES: usize = 4 + 1 + 1 + 2 + LIST_MAX_BYTES;
/// A buffer that holds any request of this protocol.
pub const REQUEST_MAX_BYTES: usize = 4 + 1 + TEXT_MAX_BYTES;

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// Write: `[C, B, ver, OP_WRITE, text_len:u8, text...]`.
    request encode_write / decode_write (op = OP_WRITE) {
        text: str8(min = 1, max = TEXT_MAX_BYTES),
    }
    /// Read: `[C, B, ver, OP_READ, seq:u64]` (0 = the newest).
    request fixed encode_read / decode_read (op = OP_READ) {
        seq: u64le,
    }
    /// List: `[C, B, ver, OP_LIST, query_len:u8, query...]` (empty = all).
    request encode_list / decode_list (op = OP_LIST) {
        query: str8(min = 0, max = QUERY_MAX_BYTES),
    }
    /// Restore: `[C, B, ver, OP_RESTORE, seq:u64]`.
    request fixed encode_restore / decode_restore (op = OP_RESTORE) {
        seq: u64le,
    }
    /// Clear: `[C, B, ver, OP_CLEAR, 0]` (one reserved byte keeps the frame non-empty).
    request fixed encode_clear / decode_clear (op = OP_CLEAR) {
        reserved: pad(1),
    }
    /// Focus push: `[C, B, ver, OP_FOCUS, focused:u64, desktop:u64, ime:u64]`.
    request fixed encode_focus / decode_focus (op = OP_FOCUS) {
        focused_sid: u64le,
        desktop_sid: u64le,
        ime_sid: u64le,
    }
    /// Status-only reply: `[C, B, ver, op|0x80, status:u8]`.
    reply fixed encode_status / decode_status (op = caller) {
        status: u8,
    }
    /// Seq reply (write / restore): `[C, B, ver, op|0x80, status:u8, seq:u64]`.
    reply fixed encode_seq_reply / decode_seq_reply (op = caller) {
        status: u8,
        seq: u64le,
    }
    /// Read reply: `[C, B, ver, op|0x80, status:u8, seq:u64, text_len:u8, text...]`.
    reply encode_read_reply / decode_read_reply (op = caller) {
        status: u8,
        seq: u64le,
        text: str8(min = 0, max = TEXT_MAX_BYTES),
    }
    /// List reply: `[C, B, ver, op|0x80, status:u8, count:u8, packed_len:u16, packed...]`
    /// — entries per [`pack_list_entry`], newest first.
    reply encode_list_reply / decode_list_reply (op = caller) {
        status: u8,
        count: u8,
        packed: bytes16(min = 0, max = LIST_MAX_BYTES),
    }
}

/// The request op of a frame in this envelope (`None` = not ours / bad header).
#[must_use]
pub fn decode_request_op(frame: &[u8]) -> Option<u8> {
    crate::codec::request_op(frame, MAGIC0, MAGIC1, VERSION)
}

/// One history entry as the list reply carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListEntry<'a> {
    /// Monotonic sequence number (newest = largest); the handle for read/restore.
    pub seq: u64,
    /// Kernel service id of the writer (who copied it).
    pub writer_sid: u64,
    /// Origin device (0 = this device; a later sync consumer fills it).
    pub device: u64,
    /// The first [`PREVIEW_MAX_BYTES`] of the text, cut on a char boundary.
    pub preview: &'a str,
}

/// Appends one entry to `out` at `len`; `None` when it does not fit or the
/// preview exceeds [`PREVIEW_MAX_BYTES`]. Returns the new length.
#[must_use]
pub fn pack_list_entry(out: &mut [u8], len: usize, e: &ListEntry<'_>) -> Option<usize> {
    let p = e.preview.as_bytes();
    let end = len.checked_add(LIST_ENTRY_HEAD)?.checked_add(p.len())?;
    if p.len() > PREVIEW_MAX_BYTES || end > out.len() {
        return None;
    }
    out[len..len + 8].copy_from_slice(&e.seq.to_le_bytes());
    out[len + 8..len + 16].copy_from_slice(&e.writer_sid.to_le_bytes());
    out[len + 16..len + 24].copy_from_slice(&e.device.to_le_bytes());
    out[len + 24] = p.len() as u8;
    out[len + LIST_ENTRY_HEAD..end].copy_from_slice(p);
    Some(end)
}

/// Walks the packed list. Fail-closed: a malformed entry ends the walk, so the
/// caller sees fewer entries than `count` — never a garbage entry.
pub fn unpack_list_entries(packed: &[u8]) -> impl Iterator<Item = ListEntry<'_>> + '_ {
    let mut at = 0usize;
    core::iter::from_fn(move || {
        let head = packed.get(at..at.checked_add(LIST_ENTRY_HEAD)?)?;
        let word = |i: usize| {
            let mut w = [0u8; 8];
            w.copy_from_slice(&head[i..i + 8]);
            u64::from_le_bytes(w)
        };
        let n = usize::from(head[24]);
        let start = at + LIST_ENTRY_HEAD;
        let Some(bytes) = packed.get(start..start + n).filter(|_| n <= PREVIEW_MAX_BYTES) else {
            at = packed.len();
            return None;
        };
        let Ok(preview) = core::str::from_utf8(bytes) else {
            at = packed.len();
            return None;
        };
        at = start + n;
        Some(ListEntry { seq: word(0), writer_sid: word(8), device: word(16), preview })
    })
}

/// The longest prefix of `text` within `max` bytes, cut on a char boundary.
fn head(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

/// A preview of `text`: at most [`PREVIEW_MAX_BYTES`] bytes, cut on a char boundary.
#[must_use]
pub fn preview_of(text: &str) -> &str {
    head(text, PREVIEW_MAX_BYTES)
}

/// What one item holds of `text`: all of it when it fits [`TEXT_MAX_BYTES`], else its head
/// cut on a char boundary. A copy of a longer selection stores the head; a cut must then
/// keep the selection (the clipboard does not hold it whole — removing it would lose text).
#[must_use]
pub fn item_text(text: &str) -> &str {
    head(text, TEXT_MAX_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_replies_round_trip() {
        let mut buf = [0u8; REQUEST_MAX_BYTES];
        let n = encode_write("Hallo", &mut buf).expect("encodes");
        assert_eq!(&buf[..4], &[b'C', b'B', 1, OP_WRITE], "envelope");
        assert_eq!(decode_request_op(&buf[..n]), Some(OP_WRITE));
        assert_eq!(decode_write(&buf[..n]), Some("Hallo"));
        assert_eq!(decode_read(&encode_read(7)), Some(7));
        assert_eq!(decode_restore(&encode_restore(9)), Some(9));
        let n = encode_list("tre", &mut buf).expect("encodes");
        assert_eq!(decode_list(&buf[..n]), Some("tre"));
        let n = encode_list("", &mut buf).expect("an empty query lists everything");
        assert_eq!(decode_list(&buf[..n]), Some(""));
        assert!(decode_clear(&encode_clear()).is_some());
        assert_eq!(decode_focus(&encode_focus(1, 2, 3)), Some((1, 2, 3)));
        let r = encode_seq_reply(OP_WRITE, STATUS_OK, 5);
        assert_eq!(decode_seq_reply(OP_WRITE, &r), Some((STATUS_OK, 5)));
        assert_eq!(
            decode_status(OP_CLEAR, &encode_status(OP_CLEAR, STATUS_DENIED)),
            Some(STATUS_DENIED)
        );
        let mut r = [0u8; REPLY_MAX_BYTES];
        let n = encode_read_reply(OP_READ, STATUS_OK, 5, "x", &mut r).expect("encodes");
        assert_eq!(decode_read_reply(OP_READ, &r[..n]), Some((STATUS_OK, 5, "x")));
    }

    #[test]
    fn test_reject_bounds_truncation_and_foreign_frames() {
        let mut buf = [0u8; REQUEST_MAX_BYTES + 8];
        assert_eq!(encode_write("", &mut buf), None, "empty item");
        let long = "x".repeat(TEXT_MAX_BYTES + 1);
        assert_eq!(encode_write(&long, &mut buf), None, "over the text bound");
        let long_query = "q".repeat(QUERY_MAX_BYTES + 1);
        assert_eq!(encode_list(&long_query, &mut buf), None, "over the query bound");
        let n = encode_write("ab", &mut buf).expect("encodes");
        assert_eq!(decode_write(&buf[..n - 1]), None, "truncated");
        assert_eq!(decode_read(&buf[..n]), None, "op mismatch");
        assert_eq!(decode_focus(&encode_read(1)), None, "focus from a read frame");
        let mut foreign = encode_read(1);
        foreign[0] = b'I';
        assert_eq!(decode_request_op(&foreign), None, "another protocol's envelope");
        // A lying length byte is not a short text.
        let mut lie = [0u8; 8];
        lie[..4].copy_from_slice(&[b'C', b'B', 1, OP_WRITE]);
        lie[4] = 200;
        assert_eq!(decode_write(&lie), None);
    }

    #[test]
    fn list_packs_and_unpacks_newest_first_and_stops_on_garbage() {
        let mut packed = [0u8; LIST_MAX_BYTES];
        let mut len = 0;
        let full = "p".repeat(PREVIEW_MAX_BYTES);
        for seq in (1..=HISTORY_MAX as u64).rev() {
            let e = ListEntry { seq, writer_sid: 40 + seq, device: 0, preview: &full };
            len = pack_list_entry(&mut packed, len, &e).expect("the whole history fits");
        }
        let mut reply = [0u8; REPLY_MAX_BYTES];
        let n =
            encode_list_reply(OP_LIST, STATUS_OK, HISTORY_MAX as u8, &packed[..len], &mut reply)
                .expect("encodes");
        let (status, count, body) = decode_list_reply(OP_LIST, &reply[..n]).expect("decodes");
        assert_eq!((status, usize::from(count)), (STATUS_OK, HISTORY_MAX));
        let entries: Vec<_> = unpack_list_entries(body).collect();
        assert_eq!(entries.len(), HISTORY_MAX);
        assert_eq!(entries[0].seq, HISTORY_MAX as u64, "newest first");
        assert_eq!(entries[0].writer_sid, 40 + HISTORY_MAX as u64);
        assert_eq!(entries[0].preview.len(), PREVIEW_MAX_BYTES);
        // A lying length byte ends the walk without a garbage entry.
        let mut bad = packed;
        bad[24] = 200;
        assert_eq!(unpack_list_entries(&bad[..len]).count(), 0);
        // Invalid UTF-8 ends it too.
        let mut bad = packed;
        bad[LIST_ENTRY_HEAD] = 0xFF;
        assert_eq!(unpack_list_entries(&bad[..len]).count(), 0);
        // An over-long preview never packs, and nothing packs past the buffer.
        let too_long = "y".repeat(PREVIEW_MAX_BYTES + 1);
        let e = ListEntry { seq: 1, writer_sid: 1, device: 0, preview: &too_long };
        assert!(pack_list_entry(&mut packed, 0, &e).is_none());
        let e = ListEntry { seq: 1, writer_sid: 1, device: 0, preview: "z" };
        assert!(pack_list_entry(&mut packed, LIST_MAX_BYTES - 1, &e).is_none());
    }

    #[test]
    fn preview_cuts_on_a_char_boundary() {
        let s = "ä".repeat(PREVIEW_MAX_BYTES); // 2 bytes each
        let p = preview_of(&s);
        assert!(p.len() <= PREVIEW_MAX_BYTES && p.len() % 2 == 0);
        assert_eq!(preview_of("short"), "short");
    }

    #[test]
    fn item_text_keeps_what_fits_and_cuts_the_rest_on_a_char_boundary() {
        assert_eq!(item_text("kiwi"), "kiwi", "a short text is held whole");
        let exact = "x".repeat(TEXT_MAX_BYTES);
        assert_eq!(item_text(&exact).len(), TEXT_MAX_BYTES, "the bound itself fits");
        let long = "ä".repeat(TEXT_MAX_BYTES); // 2 bytes each
        let held = item_text(&long);
        assert!(held.len() <= TEXT_MAX_BYTES && held.len() % 2 == 0, "never splits a char");
        assert!(held.len() < long.len(), "a longer text is held cut short");
        let mut buf = [0u8; REQUEST_MAX_BYTES];
        assert!(encode_write(held, &mut buf).is_some(), "what an item holds always encodes");
    }
}
