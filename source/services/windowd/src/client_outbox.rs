// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The compositor never waits on a client (ADR-0075, TASK-0339). A frame for an app
//! or the desktop goes straight into the client's event queue when nothing of that client waits
//! before it; otherwise — the queue is full because the client is busy, rendering its first
//! frame — the frame waits HERE, in the client's bounded outbox, and leaves in order as the
//! client drains (the kernel says when: a waitset member armed with the channel's SEND
//! capability, RFC-0033's amendment). Before this windowd parked on the client's queue for
//! every ack, tap and scroll position (`Delivery::Critical`): an app rendering for half a
//! second froze the compositor — no frame, no pointer, no other window — for that half second.
//!
//! Two kinds of frame: ORDERED — an ack the client waits for, a tap, a key, a wheel step, a
//! settings push — is never merged and never dropped while the client lives; LATEST — state the
//! next frame supersedes: the pointer over the window, a drag's position, the scroll position,
//! a frame pulse, a presentation snapshot — keeps one pending frame per kind, the newest
//! replacing the older. Frames leave in the order they were queued, a LATEST frame at the
//! place of its last update, so a client never sees a state older than an event queued after it.
//!
//! The storage is allocated once (`Outboxes::new`, at the compositor's start) and reused: a
//! client's outbox is a pool entry bound to its channel while frames wait, nothing per frame.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the unit tests below (order, coalescing, a full client, a dead client, the
//!   bounds — `test_reject_*`); the compositor's line `clients(behind= queued_max= dropped=)`
//! ADR: docs/adr/0075-the-compositor-never-waits-on-a-client.md

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

/// The largest frame windowd sends a client: the IME's candidate page.
pub(crate) const FRAME_MAX: usize = nexus_display_proto::surface_text::SURFACE_IME_STATE_FRAME_MAX;
/// ORDERED frames one client keeps before it is far behind (a refused frame is counted, and
/// named on the compositor's line — an app that drains nothing for that long is hung).
pub(crate) const ORDERED_MAX: usize = 16;
/// LATEST kinds one client keeps (pointer, drag, scroll, frame pulse, presentation, …); a kind
/// beyond them queues as ORDERED — never lost.
pub(crate) const LATEST_MAX: usize = 8;
/// Clients with an outbox at once: windowd's event-channel table holds eight channels (the
/// desktop's among them), so eight outboxes serve every client at once — ~6.75 KiB each
/// (24 slots of the largest frame), allocated once.
pub(crate) const CLIENTS_MAX: usize = 8;

/// What a frame is to its client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Delivered in order, never merged, never dropped while the client lives.
    Ordered,
    /// State: one pending frame per key, the newest replacing the older.
    Latest(u16),
}

/// One non-blocking send into the client's queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Attempt {
    Sent,
    /// The queue is full: the client is behind (the kernel can say when it has room).
    Full,
    /// The receiver's byte budget is full while its queue has room: retried on the next pass
    /// (the writable signal reports queue room, so arming it here would only spin).
    Budget,
    /// No receiver any more: the client is gone.
    Dead,
}

/// What a flush came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flush {
    /// Nothing waits any more.
    Drained,
    /// The client's queue is full again; the rest waits (arm the writable member).
    Full,
    /// The client is gone; what waited is dropped with it.
    Dead,
}

#[derive(Clone)]
struct Slot {
    /// The order the frame was queued in (a LATEST frame: its last update).
    seq: u64,
    /// `0` = empty.
    len: u16,
    key: u16,
    bytes: [u8; FRAME_MAX],
}

impl Slot {
    const EMPTY: Self = Self { seq: 0, len: 0, key: 0, bytes: [0; FRAME_MAX] };

    fn set(&mut self, seq: u64, key: u16, frame: &[u8]) {
        self.seq = seq;
        self.key = key;
        self.len = frame.len() as u16;
        self.bytes[..frame.len()].copy_from_slice(frame);
    }

    fn frame(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

/// One client's frames waiting for room in its queue.
pub(crate) struct Outbox {
    ordered: Box<[Slot]>,
    head: usize,
    count: usize,
    latest: Box<[Slot]>,
    seq: u64,
    /// ORDERED frames refused because the client was far behind.
    dropped: u32,
}

impl Outbox {
    pub(crate) fn new() -> Self {
        let slots = |n: usize| (0..n).map(|_| Slot::EMPTY).collect::<Vec<_>>().into_boxed_slice();
        Self {
            ordered: slots(ORDERED_MAX),
            head: 0,
            count: 0,
            latest: slots(LATEST_MAX),
            seq: 0,
            dropped: 0,
        }
    }

    /// Frames waiting.
    pub(crate) fn queued(&self) -> usize {
        self.count + self.latest.iter().filter(|s| s.len != 0).count()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.queued() == 0
    }

    /// ORDERED frames refused since the outbox was bound.
    pub(crate) fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Everything dropped (the client is gone, or the channel is rebound).
    pub(crate) fn clear(&mut self) {
        self.head = 0;
        self.count = 0;
        for s in self.latest.iter_mut() {
            s.len = 0;
        }
        self.dropped = 0;
    }

    /// Queues `frame`. `false`: refused — longer than any event frame, empty, or an ORDERED
    /// frame while `ORDERED_MAX` wait (counted in `dropped`).
    pub(crate) fn push(&mut self, frame: &[u8], kind: Kind) -> bool {
        if frame.is_empty() || frame.len() > FRAME_MAX {
            return false;
        }
        self.seq += 1;
        let seq = self.seq;
        if let Kind::Latest(key) = kind {
            let pending = self.latest.iter().position(|s| s.len != 0 && s.key == key);
            if let Some(i) = pending.or_else(|| self.latest.iter().position(|s| s.len == 0)) {
                self.latest[i].set(seq, key, frame);
                return true;
            }
        }
        if self.count == ORDERED_MAX {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        let at = (self.head + self.count) % ORDERED_MAX;
        self.ordered[at].set(seq, 0, frame);
        self.count += 1;
        true
    }

    /// The next frame to leave: the oldest ORDERED one or the LATEST one updated before it.
    fn next(&self) -> Option<Next> {
        let ordered = (self.count > 0).then(|| (self.ordered[self.head].seq, Next::Ordered));
        let latest = self
            .latest
            .iter()
            .enumerate()
            .filter(|(_, s)| s.len != 0)
            .map(|(i, s)| (s.seq, Next::Latest(i)))
            .min_by_key(|(seq, _)| *seq);
        match (ordered, latest) {
            (Some(o), Some(l)) => Some(if l.0 < o.0 { l.1 } else { o.1 }),
            (o, l) => o.or(l).map(|(_, n)| n),
        }
    }

    /// Sends what waits, in order, until the client's queue is full or nothing waits.
    pub(crate) fn flush(&mut self, mut send: impl FnMut(&[u8]) -> Attempt) -> Flush {
        while let Some(next) = self.next() {
            let frame = match next {
                Next::Ordered => self.ordered[self.head].frame(),
                Next::Latest(i) => self.latest[i].frame(),
            };
            match send(frame) {
                Attempt::Sent => match next {
                    Next::Ordered => {
                        self.head = (self.head + 1) % ORDERED_MAX;
                        self.count -= 1;
                    }
                    Next::Latest(i) => self.latest[i].len = 0,
                },
                Attempt::Full | Attempt::Budget => return Flush::Full,
                Attempt::Dead => {
                    self.clear();
                    return Flush::Dead;
                }
            }
        }
        Flush::Drained
    }
}

#[derive(Clone, Copy)]
enum Next {
    Ordered,
    Latest(usize),
}

/// The pool: one outbox per client channel (its SEND capability's slot) while frames wait.
pub(crate) struct Outboxes {
    boxes: Vec<(Option<u32>, Outbox)>,
    /// The compositor loop's waitset: a full channel's writable signal is armed in it.
    waitset: Option<u32>,
}

impl Outboxes {
    /// Allocates the pool once.
    pub(crate) fn new() -> Self {
        Self { boxes: (0..CLIENTS_MAX).map(|_| (None, Outbox::new())).collect(), waitset: None }
    }

    pub(crate) fn set_waitset(&mut self, waitset: u32) {
        self.waitset = Some(waitset);
    }

    pub(crate) fn waitset(&self) -> Option<u32> {
        self.waitset
    }

    /// The outbox bound to `channel`, if frames wait for it.
    pub(crate) fn get_mut(&mut self, channel: u32) -> Option<&mut Outbox> {
        self.boxes.iter_mut().find(|(c, _)| *c == Some(channel)).map(|(_, o)| o)
    }

    /// Whether frames wait for `channel` (a new frame must queue behind them).
    pub(crate) fn waiting(&self, channel: u32) -> bool {
        self.boxes.iter().any(|(c, o)| *c == Some(channel) && !o.is_empty())
    }

    /// The outbox for `channel`, binding a free one; `None` when every outbox is bound.
    pub(crate) fn bind(&mut self, channel: u32) -> Option<&mut Outbox> {
        let i = match self.boxes.iter().position(|(c, _)| *c == Some(channel)) {
            Some(i) => i,
            None => {
                let i = self.boxes.iter().position(|(c, o)| c.is_none() || o.is_empty())?;
                self.boxes[i].0 = Some(channel);
                self.boxes[i].1.clear();
                i
            }
        };
        Some(&mut self.boxes[i].1)
    }

    /// `channel` closed: what waited for it is dropped and its outbox is free.
    pub(crate) fn forget(&mut self, channel: u32) {
        for (c, o) in self.boxes.iter_mut().filter(|(c, _)| *c == Some(channel)) {
            o.clear();
            *c = None;
        }
    }

    /// The channels with frames waiting.
    pub(crate) fn behind(&self) -> impl Iterator<Item = u32> + '_ {
        self.boxes.iter().filter(|(_, o)| !o.is_empty()).filter_map(|(c, _)| *c)
    }

    /// `(clients behind, the most frames one waits with, ORDERED frames refused)` — the line.
    pub(crate) fn census(&self) -> (usize, usize, u32) {
        self.boxes.iter().filter(|(c, _)| c.is_some()).fold((0, 0, 0), |(n, max, d), (_, o)| {
            let q = o.queued();
            (n + usize::from(q != 0), max.max(q), d.saturating_add(o.dropped()))
        })
    }
}

/// The LATEST key of a client frame: its op, and for input the kind's group (a pointer's move
/// and leave are one state: where the pointer is). `None`: not an envelope frame.
pub(crate) fn latest_key(frame: &[u8]) -> Option<u16> {
    use nexus_display_proto::client_surface as wire;
    let op = *frame.get(3)?;
    let group = if op == wire::OP_SURFACE_INPUT {
        match *frame.get(8)? {
            wire::INPUT_KIND_LEAVE => wire::INPUT_KIND_MOVE,
            kind => kind,
        }
    } else {
        0
    };
    Some(u16::from(op) << 8 | u16::from(group))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn f(tag: u8) -> Vec<u8> {
        vec![tag; 13]
    }

    /// A client with room for `room` frames, recording what it got.
    fn client(room: usize, got: &mut Vec<u8>) -> impl FnMut(&[u8]) -> Attempt + '_ {
        let mut left = room;
        move |frame| {
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
        assert_eq!(o.flush(|_| Attempt::Budget), Flush::Full);
        assert_eq!(o.queued(), 1);
    }

    #[test]
    fn a_dead_client_drops_what_waited() {
        let mut o = Outbox::new();
        o.push(&f(1), Kind::Ordered);
        o.push(&f(2), Kind::Latest(1));
        assert_eq!(o.flush(|_| Attempt::Dead), Flush::Dead);
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
        // Every outbox bound and waiting: a further client gets none (the caller sends once).
        for c in 0..CLIENTS_MAX as u32 {
            pool.bind(100 + c).map(|o| o.push(&f(1), Kind::Ordered));
        }
        assert!(pool.bind(999).is_none());
        // A drained outbox is free for the next client without a forget.
        let _ = pool.get_mut(100).map(|o| o.flush(|_| Attempt::Sent));
        assert!(pool.bind(999).is_some());
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
}
