// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The present-ack window (RFC-0093 §5, TASK-0324 P6-b). Every present carries a
//! strictly increasing `seq`; gpud echoes it in the ack. A credit exists ONLY for a seq that is
//! outstanding — an unknown or already-acked seq is named, never credited, never "forgotten".
//! This replaces the in-flight LEASE: `frames_in_flight` used to be a counter that any 5-byte
//! `STATUS_OK` reply decremented, with a wall-clock lease that reset it to zero when acks went
//! quiet — a "credit without evidence" path that could never tell a lost ack from a slow one.
//! OWNERS: @ui @runtime
//! STATUS: Production (host-tested)
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (`cargo test -p windowd`). A crate-root module like
//! `presentation_state`: the compositor is OS-only, and a negative test that never compiles
//! proves nothing.

/// Presents that may be outstanding at once (RFC-0093 §5: "window ≤ 64 outstanding").
pub(crate) const WINDOW: usize = 64;

/// What an ack did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The seq was outstanding: one credit returned.
    Credited,
    /// The seq was never issued, or lies outside the window.
    Unknown,
    /// The seq was issued and already acked once.
    Duplicate,
}

/// Sequence-tracked present accounting.
#[derive(Debug, Clone)]
pub(crate) struct PresentWindow {
    /// Last seq handed to a present (0 = none yet; seqs start at 1).
    last_issued: u32,
    /// Bit `i` set ⇔ seq `base + i` is outstanding, where `base = last_issued − WINDOW + 1`
    /// clamped to 1. Presents older than the window are, by construction, acked or lost.
    outstanding: u64,
    /// Seqs at or below this were issued before the last client reset: nothing on a client
    /// that is gone can be acked, so an ack for them is `Unknown` — never a stale credit.
    floor: u32,
    /// Whether `OP_REVEAL` was sent, so a `STATUS_REVEALED` ack is expected exactly once.
    reveal_requested: bool,
    reveal_seen: bool,
}

impl PresentWindow {
    pub(crate) const fn new() -> Self {
        Self {
            last_issued: 0,
            outstanding: 0,
            floor: 0,
            reveal_requested: false,
            reveal_seen: false,
        }
    }

    /// Seq the NEXT present will carry.
    pub(crate) fn next_seq(&self) -> u32 {
        self.last_issued.wrapping_add(1).max(1)
    }

    /// Record that a present with `next_seq()` was sent. `None` when the window is full — the
    /// caller keeps its damage pending and retries on the next tick (backpressure, not a drop).
    pub(crate) fn issue(&mut self) -> Option<u32> {
        if self.in_flight() >= WINDOW as u32 {
            return None;
        }
        let seq = self.next_seq();
        self.outstanding <<= 1;
        self.outstanding |= 1;
        self.last_issued = seq;
        Some(seq)
    }

    fn slot(&self, seq: u32) -> Option<u32> {
        if seq <= self.floor || seq > self.last_issued {
            return None;
        }
        let age = self.last_issued - seq;
        (age < WINDOW as u32).then_some(age)
    }

    /// Apply an ack for `seq`.
    pub(crate) fn ack(&mut self, seq: u32) -> Verdict {
        let Some(age) = self.slot(seq) else {
            return Verdict::Unknown;
        };
        let bit = 1u64 << age;
        if self.outstanding & bit == 0 {
            return Verdict::Duplicate;
        }
        self.outstanding &= !bit;
        Verdict::Credited
    }

    /// Presents sent and not yet acked.
    pub(crate) fn in_flight(&self) -> u32 {
        self.outstanding.count_ones()
    }

    pub(crate) fn last_issued(&self) -> u32 {
        self.last_issued
    }

    /// windowd sent `OP_REVEAL`: the next `STATUS_REVEALED` ack is legitimate.
    pub(crate) fn request_reveal(&mut self) {
        self.reveal_requested = true;
    }

    /// A `STATUS_REVEALED` ack arrived. `true` exactly once, and only after a request — a
    /// reveal nobody asked for is a protocol violation, not a reveal.
    pub(crate) fn take_reveal(&mut self) -> bool {
        if !self.reveal_requested || self.reveal_seen {
            return false;
        }
        self.reveal_seen = true;
        true
    }

    /// The gpud client was reset: nothing issued so far can be acked any more — a late ack
    /// for a pre-reset seq is `Unknown`, not a credit through the side door. Seqs keep
    /// climbing; nothing is ever reused.
    pub(crate) fn reset(&mut self) {
        self.outstanding = 0;
        self.floor = self.last_issued;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_ack_roundtrip_credits_exactly_once() {
        let mut w = PresentWindow::new();
        let a = w.issue().unwrap();
        let b = w.issue().unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(w.in_flight(), 2);
        assert_eq!(w.ack(1), Verdict::Credited);
        assert_eq!(w.in_flight(), 1);
        assert_eq!(w.ack(2), Verdict::Credited);
        assert_eq!(w.in_flight(), 0);
    }

    /// An ack for a seq that was never issued is named, never credited — the class of "any
    /// 5-byte OK reply is a present completion" that corrupted the in-flight count.
    #[test]
    fn test_reject_unknown_ack_seq() {
        let mut w = PresentWindow::new();
        assert_eq!(w.ack(1), Verdict::Unknown, "nothing issued yet");
        w.issue();
        assert_eq!(w.ack(2), Verdict::Unknown, "ahead of the last issued");
        assert_eq!(w.ack(0xC0DE_0001), Verdict::Unknown, "a cursor magic is never a present");
        assert_eq!(w.in_flight(), 1, "unknown acks never touch the credit");
    }

    #[test]
    fn test_reject_duplicate_ack() {
        let mut w = PresentWindow::new();
        w.issue();
        assert_eq!(w.ack(1), Verdict::Credited);
        assert_eq!(w.ack(1), Verdict::Duplicate);
        assert_eq!(w.in_flight(), 0, "a duplicate does not go negative");
    }

    /// The window bounds outstanding presents at 64; issuing more is backpressure.
    #[test]
    fn window_bounds_outstanding_presents() {
        let mut w = PresentWindow::new();
        for _ in 0..WINDOW {
            assert!(w.issue().is_some());
        }
        assert_eq!(w.issue(), None, "65th present is refused, not silently dropped");
        assert_eq!(w.ack(1), Verdict::Credited, "the oldest is still in the window");
        assert!(w.issue().is_some(), "a credit frees a slot");
        assert_eq!(w.ack(1), Verdict::Unknown, "seq 1 has now aged out of the window");
    }

    /// After the gpud client is reset nothing outstanding can be acked: a late ack for a seq
    /// issued BEFORE the reset must not credit — that would be the "credit without evidence"
    /// path back in through the side door. The seq counter keeps climbing (never reused).
    #[test]
    fn test_reject_ack_after_reset() {
        let mut w = PresentWindow::new();
        w.issue();
        w.issue();
        assert_eq!(w.in_flight(), 2);
        w.reset();
        assert_eq!(w.in_flight(), 0);
        assert_eq!(w.ack(1), Verdict::Unknown, "a pre-reset seq is not outstanding any more");
        assert_eq!(w.ack(2), Verdict::Unknown);
        assert_eq!(w.issue(), Some(3), "seqs are never reused across a reset");
        assert_eq!(w.last_issued(), 3);
    }

    /// `last_issued` is the loop-cadence telemetry: presents sent, monotone.
    #[test]
    fn last_issued_tracks_the_newest_seq() {
        let mut w = PresentWindow::new();
        assert_eq!(w.last_issued(), 0);
        w.issue();
        w.issue();
        assert_eq!(w.last_issued(), 2);
        w.ack(1);
        assert_eq!(w.last_issued(), 2, "acks never move the issue counter");
    }

    /// A `STATUS_REVEALED` that nobody asked for is a violation, and a second one is too.
    #[test]
    fn test_reject_reveal_without_handshake() {
        let mut w = PresentWindow::new();
        assert!(!w.take_reveal(), "no OP_REVEAL was sent");
        w.request_reveal();
        assert!(w.take_reveal());
        assert!(!w.take_reveal(), "revealed exactly once");
    }
}
