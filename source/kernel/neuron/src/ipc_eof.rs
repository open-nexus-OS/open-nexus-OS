// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the RFC-0079 last-sender-EOF DECISION as a pure predicate, kept
//! OUT of the target-gated `ipc`/`cap` modules so its fail-safe truth table is
//! host-unit-tested (same rationale as `waitset`/`fence`/`image_allocs`). The
//! recv path feeds it three observed booleans; this function owns the rule:
//! EOF only when the receiver OPTED IN, a sender was once observed
//! (`had_sender`, monotonic latch), AND no live SEND cap remains
//! (`any_sender` scan). Any other combination BLOCKS — so a missed latch or a
//! server that never had a sender can never wrongly disconnect a live receiver.
//! OWNERS: @kernel-ipc-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the reject-matrix truth table below (host)

/// RFC-0079: whether a would-block recv should return `PeerClosed` (EOF)
/// instead of blocking. Fail-safe: false unless the receiver opted in AND the
/// endpoint had a sender AND none remains live.
///
/// - `eof_opt`: the recv passed `IPC_SYS_EOF`.
/// - `had_sender`: the endpoint's monotonic latch (a sender was observed).
/// - `any_sender`: a live SEND cap to the endpoint still exists (scan).
#[must_use]
pub fn should_disconnect(eof_opt: bool, had_sender: bool, any_sender: bool) -> bool {
    eof_opt && had_sender && !any_sender
}

/// TASK-0324 P7-b: whether a live SEND cap to an endpoint is held by anyone OTHER than the
/// endpoint's owner. An owner is never its own peer: a service keeps the SEND half of its
/// reply channel so it can clone one per exchange, and that base cap must not keep the
/// channel "connected" once the server that received the clone is gone — otherwise a dead
/// peer could never EOF a waiting client. `holders` yields `(pid, live_send_caps)` per task.
#[must_use]
pub fn foreign_sender_remains(
    owner: Option<u32>,
    holders: impl IntoIterator<Item = (u32, usize)>,
) -> bool {
    holders.into_iter().any(|(pid, count)| count > 0 && Some(pid) != owner)
}

/// What a recv does when it finds the queue empty (TASK-0333 6a). The wait mode chooses
/// between parking and `WouldBlock`; it never changes the EOF verdict — a non-blocking,
/// EOF-opted receiver whose last peer is gone learns it NOW. Before this the verdict lived in
/// the blocking branch alone: a waitset receiver (non-blocking recv after the wake) saw
/// `WouldBlock`, the latch was consumed, and the next wait parked forever on a peer that was
/// already dead (the closed window's app-host never exited, measured on the board).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyQueue {
    /// The last peer is gone and the receiver opted in: `PeerClosed`.
    PeerClosed,
    /// Nothing to read, no parking asked: `WouldBlock`.
    WouldBlock,
    /// Park until a sender writes, a peer leaves, or the deadline passes.
    Park,
}

/// The verdict for an empty queue; `should_disconnect` is the one rule inside it.
#[must_use]
pub fn empty_queue_verdict(
    nonblock: bool,
    eof_opt: bool,
    had_sender: bool,
    any_sender: bool,
) -> EmptyQueue {
    if should_disconnect(eof_opt, had_sender, any_sender) {
        EmptyQueue::PeerClosed
    } else if nonblock {
        EmptyQueue::WouldBlock
    } else {
        EmptyQueue::Park
    }
}

/// The sender's mirror of the rule (TASK-0333 6a): whether ANY live RECV capability to an
/// endpoint still exists — in a task's table or moving inside a queued message. The owner is
/// not excluded here: a receive needs a RECV capability, so when none exists nobody can ever
/// drain the queue, and a send into it is futile. The scan that finds none latches the
/// endpoint; every send after that returns `PeerClosed` instead of parking the sender on a
/// peer that is gone (windowd on a crashed app).
#[must_use]
pub fn receiver_remains(in_flight: usize, holders: impl IntoIterator<Item = usize>) -> bool {
    in_flight > 0 || holders.into_iter().any(|count| count > 0)
}

#[cfg(test)]
mod tests {
    use super::{
        empty_queue_verdict, foreign_sender_remains, receiver_remains, should_disconnect,
        EmptyQueue,
    };

    #[test]
    fn the_wait_mode_never_changes_the_eof_verdict() {
        // The one true EOF case disconnects whether the receiver parks or not.
        assert_eq!(empty_queue_verdict(true, true, true, false), EmptyQueue::PeerClosed);
        assert_eq!(empty_queue_verdict(false, true, true, false), EmptyQueue::PeerClosed);
        // No EOF: the mode decides between WouldBlock and Park.
        assert_eq!(empty_queue_verdict(true, true, true, true), EmptyQueue::WouldBlock);
        assert_eq!(empty_queue_verdict(false, true, true, true), EmptyQueue::Park);
        assert_eq!(empty_queue_verdict(true, false, true, false), EmptyQueue::WouldBlock);
    }

    #[test]
    fn test_reject_eof_before_any_sender_in_either_mode() {
        // A receiver that never had a peer blocks or would-block, never disconnects.
        assert_eq!(empty_queue_verdict(true, true, false, false), EmptyQueue::WouldBlock);
        assert_eq!(empty_queue_verdict(false, true, false, false), EmptyQueue::Park);
    }

    #[test]
    fn a_receiver_remains_while_any_recv_capability_lives() {
        assert!(receiver_remains(0, [0, 1]));
        assert!(receiver_remains(1, [0, 0]), "a RECV cap in flight will be received");
        assert!(!receiver_remains(0, [0, 0]));
        assert!(!receiver_remains(0, core::iter::empty()));
    }

    #[test]
    fn owner_is_never_its_own_peer() {
        // Only the owner holds a SEND cap (its clone base): no peer remains.
        assert!(!foreign_sender_remains(Some(7), [(7, 1)]));
        // The server still holds the moved clone: a peer remains.
        assert!(foreign_sender_remains(Some(7), [(7, 1), (9, 1)]));
        // The server dropped it (reply_and_close or exit): the owner's base does not count.
        assert!(!foreign_sender_remains(Some(7), [(7, 1), (9, 0)]));
    }

    #[test]
    fn test_reject_unowned_endpoint_counts_every_sender() {
        // No owner recorded: every live SEND cap keeps the channel connected (fail-safe —
        // never a spurious EOF on an endpoint the kernel cannot attribute).
        assert!(foreign_sender_remains(None, [(7, 1)]));
        assert!(!foreign_sender_remains(None, [(7, 0), (8, 0)]));
        assert!(!foreign_sender_remains(Some(7), core::iter::empty()));
    }

    #[test]
    fn reject_matrix_truth_table() {
        // A recv WITHOUT the EOF flag never disconnects (server-at-boot safety).
        assert!(!should_disconnect(false, true, false));
        assert!(!should_disconnect(false, false, false));
        assert!(!should_disconnect(false, true, true));

        // EOF-opted, but the endpoint NEVER had a sender → block (startup:
        // the sender has not attached yet).
        assert!(!should_disconnect(true, false, false));

        // EOF-opted, a sender is STILL live → block (not the last sender).
        assert!(!should_disconnect(true, true, true));

        // EOF-opted, had a sender, none remain → disconnect (the one true case).
        assert!(should_disconnect(true, true, false));

        // Degenerate: had_sender false but a sender is live — still block
        // (the latch will be set by this scan, EOF only once it's gone).
        assert!(!should_disconnect(true, false, true));
    }

    #[test]
    fn eof_requires_all_three() {
        // The disconnect case flips to block if ANY of the three conditions
        // is removed — the fail-safe invariant (never a spurious EOF).
        assert!(should_disconnect(true, true, false));
        assert!(!should_disconnect(false, true, false), "no opt-in → block");
        assert!(!should_disconnect(true, false, false), "never had a sender → block");
        assert!(!should_disconnect(true, true, true), "a sender remains → block");
    }
}
