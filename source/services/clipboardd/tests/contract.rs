// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0067 host proofs: the clipboard authority's whole contract through the
//! same `answer` the service loop runs — history order, eviction and dedupe,
//! the gate matrix (every `test_reject_*`), the query filter, malformed
//! frames, and that no marker ever carries contents.

use clipboardd::answer::{marker, Clipboard, Line, Outcome};
use clipboardd::history::{matches_query, History};
use nexus_wire::clipboardd as wire;

const WINDOWD: u64 = 0x5749_4e44;
const SHELL: u64 = 0x5348;
const KEYBOARD: u64 = 0x4b42;
const APP: u64 = 0xA9;
const OTHER_APP: u64 = 0xB7;
const HARNESS: u64 = 0x5345;

fn focus(c: &mut Clipboard, focused: u64) {
    let frame = wire::encode_focus(focused, SHELL, KEYBOARD);
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let (n, outcome) = c.answer(&frame, WINDOWD, &mut out);
    assert_eq!(n, 0, "a focus push is never answered");
    assert!(matches!(outcome, Outcome::Focus { .. }));
}

fn write(c: &mut Clipboard, sender: u64, text: &str) -> (u8, u64) {
    let mut req = [0u8; wire::REQUEST_MAX_BYTES];
    let n = wire::encode_write(text, &mut req).expect("encodes");
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let (m, _) = c.answer(&req[..n], sender, &mut out);
    wire::decode_seq_reply(wire::OP_WRITE, &out[..m]).expect("seq reply")
}

fn read(c: &mut Clipboard, sender: u64, seq: u64) -> (u8, u64, String) {
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let (m, _) = c.answer(&wire::encode_read(seq), sender, &mut out);
    let (s, q, t) = wire::decode_read_reply(wire::OP_READ, &out[..m]).expect("read reply");
    (s, q, t.to_string())
}

fn list(c: &mut Clipboard, sender: u64, query: &str) -> (u8, Vec<(u64, String)>) {
    let mut req = [0u8; wire::REQUEST_MAX_BYTES];
    let n = wire::encode_list(query, &mut req).expect("encodes");
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let (m, _) = c.answer(&req[..n], sender, &mut out);
    let (status, count, packed) =
        wire::decode_list_reply(wire::OP_LIST, &out[..m]).expect("list reply");
    let entries: Vec<_> =
        wire::unpack_list_entries(packed).map(|e| (e.seq, e.preview.to_string())).collect();
    assert_eq!(entries.len(), usize::from(count), "count matches the packed entries");
    (status, entries)
}

fn restore(c: &mut Clipboard, sender: u64, seq: u64) -> ((u8, u64), Outcome) {
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let (m, outcome) = c.answer(&wire::encode_restore(seq), sender, &mut out);
    (wire::decode_seq_reply(wire::OP_RESTORE, &out[..m]).expect("seq reply"), outcome)
}

#[test]
fn history_is_newest_first_dedupes_and_drops_the_oldest() {
    let mut h = History::new();
    for i in 0..wire::HISTORY_MAX + 2 {
        h.write(&format!("item {i}"), APP, 0).expect("stores");
    }
    assert_eq!(h.len(), wire::HISTORY_MAX, "bounded");
    let texts: Vec<_> = h.iter().map(|i| i.text().to_string()).collect();
    assert_eq!(texts[0], format!("item {}", wire::HISTORY_MAX + 1), "newest first");
    assert!(
        !texts.contains(&"item 0".to_string()) && !texts.contains(&"item 1".to_string()),
        "oldest dropped"
    );
    let seqs: Vec<_> = h.iter().map(|i| i.seq).collect();
    assert!(seqs.windows(2).all(|w| w[0] > w[1]), "seq strictly decreasing newest → oldest");
    // The same text again moves to the top — one copy, a fresh seq.
    let before = h.len();
    let seq = h.write("item 5", APP, 0).expect("stores");
    assert_eq!(h.len(), before);
    assert_eq!(
        h.newest().map(|i| (i.seq, i.text().to_string())),
        Some((seq, "item 5".to_string()))
    );
    assert_eq!(h.iter().filter(|i| i.text() == "item 5").count(), 1);
    // Restore re-stamps; an unknown seq is nothing.
    let oldest = h.iter().last().map(|i| i.seq).expect("non-empty");
    let new_seq = h.restore(oldest).expect("restores");
    assert!(new_seq > seq);
    assert_eq!(h.newest().map(|i| i.seq), Some(new_seq));
    assert_eq!(h.restore(9_999), None);
    h.clear();
    assert!(h.is_empty());
    assert!(h.write("after clear", APP, 0).expect("stores") > new_seq, "a seq is never reused");
}

#[test]
fn the_shell_and_the_keyboard_browse_and_restore_the_history() {
    let mut c = Clipboard::new(WINDOWD);
    for t in ["Treffen um 10", "https://example.org/nexus", "Einkaufsliste: Milch, Brot", "Danke!"]
    {
        assert_eq!(write(&mut c, HARNESS, t).0, wire::STATUS_OK, "a route holder writes");
    }
    focus(&mut c, APP);
    let (status, entries) = list(&mut c, SHELL, "");
    assert_eq!(status, wire::STATUS_OK);
    assert_eq!(
        entries.iter().map(|e| e.1.as_str()).collect::<Vec<_>>(),
        ["Danke!", "Einkaufsliste: Milch, Brot", "https://example.org/nexus", "Treffen um 10"]
    );
    // The query filters at the service, ASCII case-insensitively.
    let (_, hits) = list(&mut c, KEYBOARD, "MILCH");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].1, "Einkaufsliste: Milch, Brot");
    // Copy-back: the oldest becomes the newest; the first one after a list is the proof.
    let oldest = entries[3].0;
    let ((status, new_seq), outcome) = restore(&mut c, SHELL, oldest);
    assert_eq!(status, wire::STATUS_OK);
    assert_eq!(outcome, Outcome::Restored { seq: new_seq, proof: true });
    let (_, outcome) = restore(&mut c, KEYBOARD, entries[2].0);
    assert!(matches!(outcome, Outcome::Restored { proof: false, .. }), "the proof is said once");
    assert_eq!(
        read(&mut c, APP, 0).2,
        "https://example.org/nexus",
        "the focused app pastes the newest"
    );
    assert_eq!(read(&mut c, KEYBOARD, new_seq).2, "Treffen um 10", "the keyboard reads by seq");
}

#[test]
fn test_reject_reads_before_windowd_names_any_owner() {
    let mut c = Clipboard::new(WINDOWD);
    write(&mut c, HARNESS, "secret-ish");
    assert_eq!(read(&mut c, APP, 0).0, wire::STATUS_DENIED);
    assert_eq!(read(&mut c, SHELL, 0).0, wire::STATUS_DENIED, "no owner is known yet");
    assert_eq!(list(&mut c, SHELL, "").0, wire::STATUS_DENIED);
}

#[test]
fn test_reject_read_from_an_unfocused_app() {
    let mut c = Clipboard::new(WINDOWD);
    write(&mut c, APP, "mine");
    focus(&mut c, OTHER_APP);
    assert_eq!(read(&mut c, APP, 0).0, wire::STATUS_DENIED, "focus moved away");
    assert_eq!(read(&mut c, OTHER_APP, 0).2, "mine", "the focused one pastes");
    assert_eq!(read(&mut c, HARNESS, 0).0, wire::STATUS_DENIED, "a service is no focused window");
}

#[test]
fn test_reject_history_for_a_focused_app() {
    let mut c = Clipboard::new(WINDOWD);
    write(&mut c, APP, "one");
    focus(&mut c, APP);
    assert_eq!(list(&mut c, APP, "").0, wire::STATUS_DENIED, "an app never browses the history");
    let seq = c.history().newest().map(|i| i.seq).expect("stored");
    assert_eq!(restore(&mut c, APP, seq).0 .0, wire::STATUS_DENIED);
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let (m, outcome) = c.answer(&wire::encode_clear(), APP, &mut out);
    assert_eq!(wire::decode_status(wire::OP_CLEAR, &out[..m]), Some(wire::STATUS_DENIED));
    assert_eq!(outcome, Outcome::Denied { op: "clear" });
    assert_eq!(c.history().len(), 1, "nothing changed");
}

#[test]
fn test_reject_focus_push_from_anyone_but_windowd() {
    let mut c = Clipboard::new(WINDOWD);
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let forged = wire::encode_focus(APP, APP, APP);
    let (n, outcome) = c.answer(&forged, APP, &mut out);
    assert_eq!((n, outcome), (0, Outcome::FocusRejected));
    assert_eq!(c.truth().focused, 0, "the truth did not move");
    write(&mut c, OTHER_APP, "x");
    assert_eq!(read(&mut c, APP, 0).0, wire::STATUS_DENIED);
}

#[test]
fn test_reject_sender_zero_and_malformed_frames() {
    let mut c = Clipboard::new(WINDOWD);
    focus(&mut c, APP);
    assert_eq!(write(&mut c, 0, "nobody").0, wire::STATUS_DENIED, "sid 0 is nobody");
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let (_, outcome) = c.answer(&[b'C', b'B', 1], APP, &mut out);
    assert_eq!(outcome, Outcome::Malformed, "short header");
    let (_, outcome) = c.answer(&[b'C', b'B', 1, 0x7F], APP, &mut out);
    assert_eq!(outcome, Outcome::Unsupported, "unknown op");
    let mut lie = [0u8; 8];
    lie[..4].copy_from_slice(&[b'C', b'B', 1, wire::OP_WRITE]);
    lie[4] = 200;
    let (_, outcome) = c.answer(&lie, APP, &mut out);
    assert_eq!(outcome, Outcome::Malformed, "a lying length");
    assert!(c.history().is_empty());
}

#[test]
fn test_reject_unknown_seq() {
    let mut c = Clipboard::new(WINDOWD);
    focus(&mut c, APP);
    write(&mut c, APP, "a");
    assert_eq!(read(&mut c, SHELL, 4242).0, wire::STATUS_EMPTY);
    assert_eq!(restore(&mut c, SHELL, 4242).0 .0, wire::STATUS_EMPTY);
}

#[test]
fn markers_name_numbers_never_contents() {
    let mut c = Clipboard::new(WINDOWD);
    let mut req = [0u8; wire::REQUEST_MAX_BYTES];
    let n = wire::encode_write("do-not-log-me", &mut req).expect("encodes");
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let (_, outcome) = c.answer(&req[..n], APP, &mut out);
    let mut line = Line::new();
    assert!(marker(&outcome, &mut line));
    assert_eq!(line.as_str(), "clipboardd: write ok (seq=1)", "no content, no length");
    let (_, outcome) = c.answer(&wire::encode_read(0), APP, &mut out);
    let mut line = Line::new();
    assert!(marker(&outcome, &mut line));
    assert_eq!(line.as_str(), "clipboardd: read deny (reason=clipboard-focus)");
    assert!(!line.as_str().contains("do-not-log-me"));
}

#[test]
fn query_matching_is_ascii_case_insensitive() {
    assert!(matches_query("Hallo Welt", ""));
    assert!(matches_query("Hallo Welt", "welt"));
    assert!(matches_query("Hallo Welt", "LO W"));
    assert!(!matches_query("Hallo", "Hallo Welt"));
    assert!(matches_query("Äpfel", "Äpfel"), "non-ASCII bytes compare exactly");
}
