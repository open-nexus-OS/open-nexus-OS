// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests of the client outbox (`client_outbox.rs`, ADR-0075): order, coalescing, a full
//! client, a dead client, the bounds, a capability moving with its frame (RFC-0114 §3).

use super::*;
use alloc::vec;

fn f(tag: u8) -> Vec<u8> {
    vec![tag; 13]
}

/// A client with room for `room` frames, recording what it got.
fn client(room: usize, got: &mut Vec<u8>) -> impl FnMut(&[u8], Option<u32>) -> Attempt + '_ {
    let mut left = room;
    move |frame, _cap| {
        if left == 0 {
            return Attempt::Full;
        }
        left -= 1;
        got.push(frame[0]);
        Attempt::Sent
    }
}

#[test]
fn ordered_frames_leave_in_order_as_the_client_drains() {
    let mut o = Outbox::new();
    for t in 1..=5 {
        assert!(o.push(&f(t), Kind::Ordered));
    }
    let mut got = Vec::new();
    assert_eq!(o.flush(client(2, &mut got)), Flush::Full, "the queue fills after two");
    assert_eq!(got, [1, 2]);
    assert_eq!(o.queued(), 3, "the rest waits, nothing lost");
    let mut got = Vec::new();
    assert_eq!(o.flush(client(8, &mut got)), Flush::Drained);
    assert_eq!(got, [3, 4, 5]);
    assert!(o.is_empty());
}

#[test]
fn a_latest_frame_replaces_the_older_one_of_its_kind() {
    let mut o = Outbox::new();
    o.push(&f(1), Kind::Latest(7));
    o.push(&f(2), Kind::Latest(7));
    o.push(&f(3), Kind::Latest(9));
    assert_eq!(o.queued(), 2, "one pending per kind");
    let mut got = Vec::new();
    o.flush(client(8, &mut got));
    assert_eq!(got, [2, 3], "the newest of each kind, in the order of their last update");
}

/// A pointer move queued after a tap leaves after it; one queued before it leaves before
/// it — the client never sees a state older than an event queued after it.
#[test]
fn a_latest_frame_leaves_at_the_place_of_its_last_update() {
    let mut o = Outbox::new();
    o.push(&f(1), Kind::Latest(7)); // move to a
    o.push(&f(2), Kind::Ordered); // tap
    let mut got = Vec::new();
    o.flush(client(8, &mut got));
    assert_eq!(got, [1, 2]);
    o.push(&f(1), Kind::Latest(7)); // move to a
    o.push(&f(2), Kind::Ordered); // tap
    o.push(&f(3), Kind::Latest(7)); // move to b: replaces a, after the tap
    let mut got = Vec::new();
    o.flush(client(8, &mut got));
    assert_eq!(got, [2, 3]);
}

/// A byte-budget refusal stops the flush like a full queue; nothing is lost.
#[test]
fn a_budget_refusal_keeps_the_frame_for_the_next_pass() {
    let mut o = Outbox::new();
    o.push(&f(1), Kind::Ordered);
    assert_eq!(o.flush(|_, _| Attempt::Budget), Flush::Full);
    assert_eq!(o.queued(), 1);
}

#[test]
fn a_dead_client_drops_what_waited() {
    let mut o = Outbox::new();
    o.push(&f(1), Kind::Ordered);
    o.push(&f(2), Kind::Latest(1));
    assert_eq!(o.flush(|_, _| Attempt::Dead), Flush::Dead);
    assert!(o.is_empty());
}

#[test]
fn test_reject_an_ordered_frame_past_the_bound() {
    let mut o = Outbox::new();
    for t in 0..ORDERED_MAX as u8 {
        assert!(o.push(&f(t), Kind::Ordered));
    }
    assert!(!o.push(&f(99), Kind::Ordered), "far behind: refused, never parked");
    assert_eq!(o.dropped(), 1);
    assert_eq!(o.queued(), ORDERED_MAX, "what waited is kept");
}

#[test]
fn test_reject_a_frame_longer_than_any_event_or_empty() {
    let mut o = Outbox::new();
    assert!(!o.push(&vec![0; FRAME_MAX + 1], Kind::Ordered));
    assert!(!o.push(&[], Kind::Latest(1)));
    assert!(o.is_empty());
    assert_eq!(o.dropped(), 0, "a malformed frame is no client's backlog");
}

#[test]
fn a_latest_kind_past_the_slots_queues_as_ordered() {
    let mut o = Outbox::new();
    for k in 0..=LATEST_MAX as u16 {
        assert!(o.push(&f(k as u8), Kind::Latest(k)));
    }
    assert_eq!(o.queued(), LATEST_MAX + 1, "never lost");
    let mut got = Vec::new();
    o.flush(client(32, &mut got));
    assert_eq!(got.len(), LATEST_MAX + 1);
    assert_eq!(got, (0..=LATEST_MAX as u8).collect::<Vec<_>>(), "in order");
}

#[test]
fn the_pool_binds_reuses_and_forgets_by_channel() {
    let mut pool = Outboxes::new();
    assert_eq!(pool.waitset(), None, "no writable signal before the loop names its waitset");
    pool.set_waitset(7);
    assert_eq!(pool.waitset(), Some(7));
    assert!(!pool.waiting(4));
    pool.bind(4).map(|o| o.push(&f(1), Kind::Ordered));
    assert!(pool.waiting(4));
    assert_eq!(pool.behind().collect::<Vec<_>>(), [4]);
    assert_eq!(pool.census(), (1, 1, 0));
    pool.forget(4);
    assert!(!pool.waiting(4) && pool.get_mut(4).is_none());
    // RFC-0114 §7: every reserved outbox bound and waiting — a further client gets a new one
    // (the pool grows with the clients behind at once, no ninth client goes without).
    for c in 0..CLIENTS_RESERVED as u32 {
        pool.bind(100 + c).map(|o| o.push(&f(1), Kind::Ordered));
    }
    assert_eq!(pool.len(), CLIENTS_RESERVED);
    assert!(pool.bind(999).map(|o| o.push(&f(1), Kind::Ordered)).is_some());
    assert_eq!(pool.len(), CLIENTS_RESERVED + 1);
    assert_eq!(pool.behind().count(), CLIENTS_RESERVED + 1);
    // A drained outbox is free for the next client without a forget — reused, not grown.
    let _ = pool.get_mut(100).map(|o| o.flush(|_, _| Attempt::Sent));
    assert!(pool.bind(1000).is_some());
    assert_eq!(pool.len(), CLIENTS_RESERVED + 1);
}

fn commit(text: &str, seq: u32) -> Vec<u8> {
    use nexus_display_proto::surface_text as st;
    let stamp = st::TextStamp { seq, t_ns: u64::from(seq) * 1_000 };
    let (frame, len) =
        st::encode_surface_text(st::SURFACE_TEXT_COMMIT, 0, text, stamp).expect("encodes");
    frame[..len].to_vec()
}

fn texts(o: &mut Outbox) -> Vec<(u8, alloc::string::String, u32)> {
    use nexus_display_proto::surface_text as st;
    let mut got = Vec::new();
    o.flush(|frame, _| {
        let (kind, _, text, stamp) = st::decode_surface_text(frame).expect("a text frame");
        got.push((kind, alloc::string::String::from(text), stamp.seq));
        Attempt::Sent
    });
    got
}

/// RFC-0113: a client behind a fast typist gets the run as one frame — every character, in
/// order, the first key's stamp.
#[test]
fn consecutive_commits_join_while_the_client_is_behind() {
    let mut o = Outbox::new();
    for (i, ch) in ["f", "a", "s", "t"].iter().enumerate() {
        assert!(o.push(&commit(ch, i as u32 + 1), Kind::Ordered));
    }
    assert_eq!(o.queued(), 1, "one frame holds the run");
    assert_eq!(texts(&mut o), [(0, "fast".into(), 1)]);
}

#[test]
fn test_reject_a_join_across_an_action_or_past_the_text_bound() {
    use nexus_display_proto::surface_text as st;
    let mut o = Outbox::new();
    o.push(&commit("ab", 1), Kind::Ordered);
    let (back, len) =
        st::encode_surface_text(st::SURFACE_TEXT_ACTION, 2, "", st::TextStamp::NONE).unwrap();
    o.push(&back[..len], Kind::Ordered);
    o.push(&commit("c", 3), Kind::Ordered);
    let got = texts(&mut o);
    assert_eq!(got.len(), 3, "an action between two commits keeps them apart");
    assert_eq!(got[2].1, "c");
    let long = "x".repeat(st::SURFACE_TEXT_MAX_BYTES - 1);
    o.push(&commit(&long, 4), Kind::Ordered);
    o.push(&commit("yz", 5), Kind::Ordered);
    assert_eq!(o.queued(), 2, "the joined text would not fit one frame");
    let mut taps = Outbox::new();
    taps.push(&f(1), Kind::Ordered);
    taps.push(&f(2), Kind::Ordered);
    assert_eq!(taps.queued(), 2, "only commits join");
}

#[test]
fn the_latest_key_groups_the_pointer_and_separates_ops() {
    use nexus_display_proto::client_surface as wire;
    let mv = wire::encode_surface_input(1, wire::INPUT_KIND_MOVE, 3, 4);
    let leave = wire::encode_surface_input(1, wire::INPUT_KIND_LEAVE, 0, 0);
    let drag = wire::encode_surface_input(1, wire::INPUT_KIND_DRAG, 3, 4);
    let pulse = wire::encode_surface_frame(1);
    assert_eq!(latest_key(&mv), latest_key(&leave), "where the pointer is: one state");
    assert_ne!(latest_key(&mv), latest_key(&drag));
    assert_ne!(latest_key(&mv), latest_key(&pulse));
    assert_eq!(latest_key(&[1, 2]), None);
}

/// RFC-0113 §2: a MOVE that replaces a waiting one carries both batches' samples.
#[test]
fn a_replaced_move_hands_on_its_samples() {
    use nexus_display_proto::client_surface as wire;
    use nexus_display_proto::surface_input::{decode_surface_motion, encode_surface_motion};
    use nexus_display_proto::surface_motion::{MotionBatch, MotionSample};
    let batch = |x: i32| {
        let mut b = MotionBatch::new();
        b.push(MotionSample { x, y: 1, t_ns: x as u64 * 1_000, seq: x as u32 });
        b
    };
    let (a, n) = encode_surface_motion(7, wire::INPUT_KIND_MOVE, 3, 1, &batch(3)).expect("a");
    let (b, m) = encode_surface_motion(7, wire::INPUT_KIND_MOVE, 5, 1, &batch(5)).expect("b");
    let key = latest_key(&a[..n]).expect("key");
    let mut o = Outbox::new();
    assert!(o.push(&a[..n], Kind::Latest(key)));
    assert!(o.push(&b[..m], Kind::Latest(key)));
    let mut got = Vec::new();
    let _ = o.flush(|f, _| {
        got.push(f.to_vec());
        Attempt::Sent
    });
    assert_eq!(got.len(), 1, "one MOVE leaves");
    let (_, _, x, _, motion) = decode_surface_motion(&got[0]).expect("a motion frame");
    let mut both = batch(3);
    both.append(&batch(5));
    assert_eq!((x, motion), (5, both));
}

/// RFC-0114 §3: a surface's buffer moves with its answer — in order behind what waited before it,
/// taken by the send that delivers the frame, kept while the client's queue is full.
#[test]
fn a_capability_moves_with_its_frame_in_order() {
    let mut o = Outbox::new();
    assert!(o.push(&f(1), Kind::Ordered));
    assert_eq!(o.push_cap(&f(2), 77), Ok(()));
    assert!(o.push(&f(3), Kind::Ordered));
    assert_eq!(o.caps_waiting(), 1);
    let mut got = Vec::new();
    let full = o.flush(|frame, cap| {
        if frame[0] == 2 {
            return Attempt::Full; // the client is behind exactly at the answer
        }
        got.push((frame[0], cap));
        Attempt::Sent
    });
    assert_eq!(full, Flush::Full);
    assert_eq!(o.caps_waiting(), 1, "a full queue leaves the capability waiting");
    let _ = o.flush(|frame, cap| {
        got.push((frame[0], cap));
        Attempt::Sent
    });
    assert_eq!(got, [(1, None), (2, Some(77)), (3, None)]);
    assert_eq!(o.caps_waiting(), 0, "taken by the send");
}

/// A dead client drops every frame — and every capability one carried is closed, never left
/// in windowd's table.
#[test]
fn test_reject_keeping_a_capability_for_a_dead_client() {
    let mut o = Outbox::new();
    assert_eq!(o.push_cap(&f(1), 5), Ok(()));
    assert_eq!(o.push_cap(&f(2), 6), Ok(()));
    assert_eq!(o.flush(|_, _| Attempt::Dead), Flush::Dead);
    assert_eq!((o.queued(), o.caps_waiting()), (0, 0));
    // A full outbox refuses the frame and hands the capability back.
    for t in 0..ORDERED_MAX as u8 {
        assert!(o.push(&f(t), Kind::Ordered));
    }
    assert_eq!(o.push_cap(&f(9), 8), Err(8));
}

/// RFC-0112: a window closed on purpose gets its `OP_SURFACE_CLOSED` alone, behind nothing
/// (what waited is dropped), and the channel is released only once that frame left — or the
/// client died. Never a bare close an app would read as the compositor's death.
#[test]
fn a_closing_channel_sends_its_last_frame_alone_then_goes() {
    let mut pool = Outboxes::new();
    pool.bind(4).map(|o| o.push(&f(1), Kind::Ordered));
    pool.bind(4).map(|o| o.push(&f(2), Kind::Ordered));
    assert!(pool.push_last(4, &f(9)));
    assert!(pool.is_closing(4));
    let mut sent = Vec::new();
    let _ = pool.get_mut(4).map(|o| {
        o.flush(|frame, _| {
            sent.push(frame[4]);
            Attempt::Sent
        })
    });
    assert_eq!(sent, [9], "only the last frame: what waited before it was dropped");
    // In the runtime, `drained` closes the capability (a no-op syscall on the host).
    pool.drained(4);
    assert!(!pool.is_closing(4) && !pool.waiting(4));
    // A client that died while its last frame waited: released too.
    assert!(pool.push_last(5, &f(9)));
    pool.dead(5);
    assert!(!pool.is_closing(5) && !pool.waiting(5));
}
