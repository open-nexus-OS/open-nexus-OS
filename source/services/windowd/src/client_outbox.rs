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
//! One exception keeps every character of a fast typist (RFC-0113, TASK-0346): a committed text
//! queued right behind another one is APPENDED to it while the joined text fits a text frame —
//! not a character lost, none reordered, fewer frames for a client that is behind (it applies
//! the run as one edit). The joined frame keeps its first key's stamp; an editing action is never
//! joined. Likewise a waiting MOVE or DRAG hands its pointer samples to the one replacing it
//! (RFC-0113 §2: the position is the newest, the history keeps every sample).
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

/// The largest frame windowd sends a client (the IME's candidate page) — the one bound its
/// clients size their buffers by.
pub(crate) const FRAME_MAX: usize = nexus_display_proto::surface_text::CLIENT_EVENT_FRAME_MAX;
/// ORDERED frames one client keeps before it is far behind (a refused frame is counted, and
/// named on the compositor's line — an app that drains nothing for that long is hung).
pub(crate) const ORDERED_MAX: usize = 16;
/// LATEST kinds one client keeps (pointer, drag, scroll, frame pulse, presentation, …); a kind
/// beyond them queues as ORDERED — never lost.
pub(crate) const LATEST_MAX: usize = 8;
/// Outboxes the pool is born with — a usual session's clients, the desktop's among them — ~6.75
/// KiB each (24 slots of the largest frame). Past them the pool grows with the clients behind at
/// once (RFC-0114 §7: windowd's channels are as many as the windows); a free outbox is reused
/// first.
pub(crate) const CLIENTS_RESERVED: usize = 8;

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
    /// A capability that moves with the frame (an ORDERED frame only: a surface's buffer,
    /// RFC-0114 §3) — windowd's until the send takes it.
    cap: Option<u32>,
}

impl Slot {
    const EMPTY: Self = Self { seq: 0, len: 0, key: 0, bytes: [0; FRAME_MAX], cap: None };

    fn set(&mut self, seq: u64, key: u16, frame: &[u8]) {
        self.seq = seq;
        self.key = key;
        self.len = frame.len() as u16;
        self.bytes[..frame.len()].copy_from_slice(frame);
        self.cap = None;
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

    /// An empty outbox made while windowd runs; `None` when memory refuses its slots (the
    /// caller sends once, as for a client without an outbox).
    fn try_new() -> Option<Self> {
        let slots = |n: usize| -> Option<Box<[Slot]>> {
            let mut v = Vec::new();
            v.try_reserve_exact(n).ok()?;
            v.extend((0..n).map(|_| Slot::EMPTY));
            Some(v.into_boxed_slice())
        };
        Some(Self {
            ordered: slots(ORDERED_MAX)?,
            head: 0,
            count: 0,
            latest: slots(LATEST_MAX)?,
            seq: 0,
            dropped: 0,
        })
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

    /// Capabilities waiting to move with their frames.
    #[cfg(test)]
    pub(crate) fn caps_waiting(&self) -> usize {
        (0..self.count)
            .filter(|i| self.ordered[(self.head + i) % ORDERED_MAX].cap.is_some())
            .count()
    }

    /// Everything dropped (the client is gone, or the channel is rebound) — with every
    /// capability a waiting frame carried, closed.
    pub(crate) fn clear(&mut self) {
        for i in 0..self.count {
            if let Some(cap) = self.ordered[(self.head + i) % ORDERED_MAX].cap.take() {
                close_cap(cap);
            }
        }
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
            // A waiting MOVE or DRAG hands its pointer samples to its successor (RFC-0113 §2).
            let joined = pending.and_then(|i| {
                nexus_display_proto::surface_input::join_motion_frames(
                    self.latest[i].frame(),
                    frame,
                )
            });
            if let (Some(i), Some((bytes, n))) = (pending, joined) {
                self.latest[i].set(seq, key, &bytes[..n]);
                return true;
            }
            if let Some(i) = pending.or_else(|| self.latest.iter().position(|s| s.len == 0)) {
                self.latest[i].set(seq, key, frame);
                return true;
            }
        }
        if self.count > 0 {
            let last = (self.head + self.count - 1) % ORDERED_MAX;
            if let Some(joined) = join_commits(self.ordered[last].frame(), frame) {
                let keep = self.ordered[last].seq;
                self.ordered[last].set(keep, 0, &joined.0[..joined.1]);
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

    /// Queues `frame` ORDERED with `cap` moving along (never merged, never joined). `Err(cap)`:
    /// refused as [`Self::push`] refuses — the capability stays the caller's.
    pub(crate) fn push_cap(&mut self, frame: &[u8], cap: u32) -> Result<(), u32> {
        if frame.is_empty() || frame.len() > FRAME_MAX || self.count == ORDERED_MAX {
            self.dropped = self.dropped.saturating_add(u32::from(self.count == ORDERED_MAX));
            return Err(cap);
        }
        self.seq += 1;
        let at = (self.head + self.count) % ORDERED_MAX;
        self.ordered[at].set(self.seq, 0, frame);
        self.ordered[at].cap = Some(cap);
        self.count += 1;
        Ok(())
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

    /// Sends what waits, in order, until the client's queue is full or nothing waits. `send`
    /// gets each frame and the capability moving with it: `Sent` took the capability, a full
    /// queue left it here, a dead client took or refused it (closed either way — at once, before
    /// any other capability could take its slot).
    pub(crate) fn flush(&mut self, mut send: impl FnMut(&[u8], Option<u32>) -> Attempt) -> Flush {
        while let Some(next) = self.next() {
            let (frame, cap) = match next {
                Next::Ordered => (self.ordered[self.head].frame(), self.ordered[self.head].cap),
                Next::Latest(i) => (self.latest[i].frame(), None),
            };
            match send(frame, cap) {
                Attempt::Sent => match next {
                    Next::Ordered => {
                        self.ordered[self.head].cap = None;
                        self.head = (self.head + 1) % ORDERED_MAX;
                        self.count -= 1;
                    }
                    Next::Latest(i) => self.latest[i].len = 0,
                },
                Attempt::Full | Attempt::Budget => return Flush::Full,
                Attempt::Dead => {
                    if let (Next::Ordered, Some(cap)) = (next, self.ordered[self.head].cap.take()) {
                        close_cap(cap);
                    }
                    self.clear();
                    return Flush::Dead;
                }
            }
        }
        Flush::Drained
    }
}

/// Closes a capability a frame carried that will never move (its client is gone).
pub(crate) fn close_cap(cap: u32) {
    #[cfg(nexus_env = "os")]
    let _ = nexus_abi::cap_close(cap);
    #[cfg(not(nexus_env = "os"))]
    let _ = cap;
}

/// Two committed texts, `first` then `then`, as ONE text frame with `first`'s stamp — `None` when
/// either is not a commit or the joined text would not fit a text frame.
fn join_commits(
    first: &[u8],
    then: &[u8],
) -> Option<([u8; nexus_display_proto::surface_text::SURFACE_TEXT_FRAME_MAX], usize)> {
    use nexus_display_proto::surface_text as st;
    let (k0, aux, t0, stamp) = st::decode_surface_text(first)?;
    let (k1, _, t1, _) = st::decode_surface_text(then)?;
    if k0 != st::SURFACE_TEXT_COMMIT || k1 != st::SURFACE_TEXT_COMMIT {
        return None;
    }
    let n = t0.len() + t1.len();
    if n > st::SURFACE_TEXT_MAX_BYTES {
        return None;
    }
    let mut text = [0u8; st::SURFACE_TEXT_MAX_BYTES];
    text[..t0.len()].copy_from_slice(t0.as_bytes());
    text[t0.len()..n].copy_from_slice(t1.as_bytes());
    let text = core::str::from_utf8(&text[..n]).ok()?;
    st::encode_surface_text(st::SURFACE_TEXT_COMMIT, aux, text, stamp)
}

#[derive(Clone, Copy)]
enum Next {
    Ordered,
    Latest(usize),
}

/// The pool: one outbox per client channel (its SEND capability's slot) while frames wait.
pub(crate) struct Outboxes {
    boxes: Vec<(Option<u32>, Outbox)>,
    /// Channels whose last frame waits (`OP_SURFACE_CLOSED`, RFC-0112): closed once it left.
    closing: Vec<u32>,
    /// The compositor loop's waitset: a full channel's writable signal is armed in it.
    waitset: Option<u32>,
}

impl Outboxes {
    /// The pool with its reserved outboxes.
    pub(crate) fn new() -> Self {
        Self {
            boxes: (0..CLIENTS_RESERVED).map(|_| (None, Outbox::new())).collect(),
            closing: Vec::new(),
            waitset: None,
        }
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

    /// The outbox for `channel`, binding a free one or a new one; `None` only when memory
    /// refuses a new one.
    pub(crate) fn bind(&mut self, channel: u32) -> Option<&mut Outbox> {
        let i = match self.boxes.iter().position(|(c, _)| *c == Some(channel)) {
            Some(i) => i,
            None => {
                let i = match self.boxes.iter().position(|(c, o)| c.is_none() || o.is_empty()) {
                    Some(i) => i,
                    None => {
                        self.boxes.try_reserve(1).ok()?;
                        self.boxes.push((None, Outbox::try_new()?));
                        self.boxes.len() - 1
                    }
                };
                self.boxes[i].0 = Some(channel);
                self.boxes[i].1.clear();
                i
            }
        };
        Some(&mut self.boxes[i].1)
    }

    /// Outboxes in the pool (bound or free): the bound of [`Self::behind_at`].
    pub(crate) fn len(&self) -> usize {
        self.boxes.len()
    }

    /// The channel of outbox `i` if frames wait in it. Outboxes are never removed, so a pass
    /// over `0..len()` holds while a flush forgets a channel.
    pub(crate) fn behind_at(&self, i: usize) -> Option<u32> {
        self.boxes.get(i).filter(|(_, o)| !o.is_empty()).and_then(|(c, _)| *c)
    }

    /// `channel` closed: what waited for it is dropped and its outbox is free.
    pub(crate) fn forget(&mut self, channel: u32) {
        for (c, o) in self.boxes.iter_mut().filter(|(c, _)| *c == Some(channel)) {
            o.clear();
            *c = None;
        }
        self.closing.retain(|c| *c != channel);
    }

    /// `channel`'s LAST frame (RFC-0112: `OP_SURFACE_CLOSED`), alone: what waited for it is
    /// dropped, the frame waits in its emptied outbox, and the channel's capability is closed
    /// once the frame left (`drained`) or the client died (`dead`). `false` only when memory
    /// refuses an outbox (the caller closes the channel itself).
    pub(crate) fn push_last(&mut self, channel: u32, frame: &[u8]) -> bool {
        self.forget(channel);
        if self.closing.try_reserve(1).is_err() {
            return false;
        }
        if !self.bind(channel).is_some_and(|outbox| outbox.push(frame, Kind::Ordered)) {
            self.forget(channel);
            return false;
        }
        self.closing.push(channel);
        true
    }

    /// Everything waiting for `channel` left: a closing channel's capability goes now.
    pub(crate) fn drained(&mut self, channel: u32) {
        if self.closing.contains(&channel) {
            self.forget(channel);
            close_cap(channel);
        }
    }

    /// `channel`'s client is gone: what waited is dropped, a closing channel's capability goes.
    pub(crate) fn dead(&mut self, channel: u32) {
        let closing = self.closing.contains(&channel);
        self.forget(channel);
        if closing {
            close_cap(channel);
        }
    }

    /// Whether `channel` waits to be closed after its last frame.
    #[cfg(test)]
    pub(crate) fn is_closing(&self, channel: u32) -> bool {
        self.closing.contains(&channel)
    }

    /// The channels with frames waiting.
    #[cfg(test)]
    pub(crate) fn behind(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.len()).filter_map(|i| self.behind_at(i))
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
#[path = "client_outbox_tests.rs"]
mod tests;
