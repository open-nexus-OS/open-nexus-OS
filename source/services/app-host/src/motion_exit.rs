// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the pure decisions of a motion layer's exit (RFC-0109 §motion layers, ADR-0097
//! amendment "Exits", TASK-0354 P9): what becomes of a layer after a re-emit (it stays, the base
//! takes its subtree back, it leaves as a ghost, or it goes at once), which ghost a re-entering
//! transition revives (by its identity, never by a pre-order number), and when a ghost is dropped.
//! Same one-cfg shape as `motion_math`: the probe (`probe/motion_leave.rs`) acts, the decisions
//! live here where host tests reach them.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below

/// What becomes of a motion layer after a re-emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fate {
    /// Its transition still enters: it stays (re-resolved at its node).
    Stays,
    /// Its transition no longer enters but its subtree is still there: the base paints it again.
    BaseTakesBack,
    /// Its node left the scene and it was drawn moving: a ghost plays its exit.
    Leaves,
    /// Its node left and an exit would show nothing (reduced motion, never drawn): gone at once.
    Goes,
}

/// The fate of a layer whose transition `entering` (its identity among the transitions now) and
/// whose subtree is `mounted` (still in the scene at its place); `drawn` (its pixels were shown)
/// with a motion `duration_ms` long.
pub(crate) fn fate(entering: bool, mounted: bool, drawn: bool, duration_ms: u16) -> Fate {
    match (entering, mounted) {
        (true, _) => Fate::Stays,
        (false, true) => Fate::BaseTakesBack,
        (false, false) if drawn && duration_ms > 0 => Fate::Leaves,
        (false, false) => Fate::Goes,
    }
}

/// The slot whose ghost a transition of `identity` revives: the layer leaving with that identity
/// — never another layer that happens to sit at the node's pre-order number. `layers[k]`: slot
/// `k`'s identity and whether it is leaving.
pub(crate) fn revives(layers: &[Option<(i32, bool)>], identity: i32) -> Option<usize> {
    layers.iter().position(|l| *l == Some((identity, true)))
}

/// When a ghost that left at `now` (ns) is dropped: its exit's end — its scrim's fade when that
/// is longer — plus `over` (the margin a compositor's last frame of it may take).
pub(crate) fn ghost_until(now: u64, duration_ms: u16, scrim_ms: u16, over: u64) -> u64 {
    now.saturating_add(u64::from(duration_ms.max(scrim_ms)) * 1_000_000).saturating_add(over)
}

/// Whether a ghost to be dropped at `until` is due at `now` (it never outlives its deadline).
pub(crate) fn ghost_due(until: u64, now: u64) -> bool {
    now >= until
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    /// TASK-0354 P9: a panel whose node left the scene leaves the way it came — unless nothing
    /// would show (reduced motion, a layer never drawn); one whose subtree is still there but no
    /// longer a transition goes back to the base.
    #[test]
    fn a_layer_leaves_when_its_identity_is_gone() {
        assert_eq!(fate(true, true, true, 280), Fate::Stays);
        assert_eq!(fate(true, false, true, 280), Fate::Stays, "renumbered, still entering");
        assert_eq!(fate(false, false, true, 280), Fate::Leaves);
        assert_eq!(fate(false, true, true, 280), Fate::BaseTakesBack);
        assert_eq!(fate(false, false, true, 0), Fate::Goes, "reduced motion leaves at once");
        assert_eq!(fate(false, false, false, 280), Fate::Goes, "never drawn: nothing to play");
    }

    /// The identity decides, not the pre-order number: another node landing on the number a
    /// leaving layer had does not take its ghost.
    #[test]
    fn another_node_on_the_same_number_does_not_revive_it() {
        let layers = [Some((0x51, true)), None, Some((0x77, false))];
        assert_eq!(revives(&layers, 0x99), None, "another node");
        assert_eq!(revives(&layers, 0x77), None, "a layer that is not leaving is not revived");
    }

    /// The same transition coming back while its layer leaves turns that layer back (windowd
    /// keeps its id, its place and its velocity).
    #[test]
    fn a_reopen_within_the_exit_revives_the_same_id() {
        let layers = [None, Some((0x51, true)), Some((0x51, false))];
        assert_eq!(revives(&layers, 0x51), Some(1), "the leaving one, not the live twin");
    }

    /// A ghost is dropped at its exit's end (its scrim's if longer) plus the margin — never later.
    #[test]
    fn test_reject_keeping_a_ghost_past_its_deadline() {
        let until = ghost_until(1_000 * MS, 280, 160, 250 * MS);
        assert_eq!(until, (1_000 + 280 + 250) * MS);
        assert_eq!(ghost_until(0, 120, 200, 0), 200 * MS, "the scrim's fade is longer");
        assert!(!ghost_due(until, until - 1));
        assert!(ghost_due(until, until), "due at its deadline");
        assert!(ghost_due(until, until + 5 * MS));
        assert_eq!(ghost_until(u64::MAX - 1, 1000, 0, u64::MAX), u64::MAX, "saturates");
    }
}
