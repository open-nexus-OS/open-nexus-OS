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

#[cfg(test)]
mod tests {
    use super::should_disconnect;

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
