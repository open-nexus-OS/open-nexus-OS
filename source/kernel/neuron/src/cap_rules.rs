// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The capability-transfer rules of RFC-0112 (amendment 2026-11-02) as pure predicates
//! — host-tested here, applied by `syscall::api::caps` (which, like `cap` and `ipc`, compiles for
//! the target only). A capability goes only into the caller itself or one of its direct
//! children: the parent that wires a child (init → its services, execd → its apps, a work pool
//! → its threads); into a stranger it would take an empty slot the stranger's parent was about
//! to pin a route into — a forged route. A REPLACE_DEAD transfer re-pins a slot that is empty or
//! holds a capability to a DEAD endpoint (a restarted server's route into its clients'
//! declared slots), never a live capability or one of another kind.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below; `KSELFTEST: cap transfer rules ok (` at boot (the syscalls)

/// Whether `caller` may transfer a capability into `child`, whose parent is `child_parent`.
#[must_use]
pub fn transfer_target_allowed(caller: u32, child: u32, child_parent: Option<u32>) -> bool {
    child == caller || child_parent == Some(caller)
}

/// What a REPLACE_DEAD transfer finds in its target slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotHolds {
    Empty,
    /// A capability to an endpoint, alive or dead.
    Endpoint {
        alive: bool,
    },
    /// A capability of any other kind.
    Other,
}

/// What a REPLACE_DEAD transfer does with that slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplaceVerdict {
    /// The slot is empty: an ordinary pin.
    Pin,
    /// The dead capability is dropped and replaced.
    ReplaceDead,
    /// Refused (`EPERM`): a live capability is never overwritten.
    Refuse,
}

/// REPLACE_DEAD's rule.
#[must_use]
pub fn replace_dead_verdict(slot: SlotHolds) -> ReplaceVerdict {
    match slot {
        SlotHolds::Empty => ReplaceVerdict::Pin,
        SlotHolds::Endpoint { alive: false } => ReplaceVerdict::ReplaceDead,
        SlotHolds::Endpoint { alive: true } | SlotHolds::Other => ReplaceVerdict::Refuse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transfer_goes_into_the_caller_or_its_child() {
        assert!(transfer_target_allowed(1, 1, Some(0)), "itself");
        assert!(transfer_target_allowed(1, 7, Some(1)), "its child");
        assert!(transfer_target_allowed(0, 1, Some(0)), "the bootstrap task into init");
    }

    /// RFC-0112 (amendment 2026-11-02, point 1): no capability into a stranger — a sibling, a
    /// grandchild, the parent, a task without a parent.
    #[test]
    fn test_reject_cap_transfer_into_a_stranger() {
        assert!(!transfer_target_allowed(7, 8, Some(1)), "a sibling");
        assert!(!transfer_target_allowed(1, 9, Some(7)), "a grandchild");
        assert!(!transfer_target_allowed(7, 1, Some(0)), "the parent");
        assert!(!transfer_target_allowed(7, 0, None), "the bootstrap task");
    }

    #[test]
    fn replace_dead_pins_an_empty_slot_and_replaces_a_dead_endpoint() {
        assert_eq!(replace_dead_verdict(SlotHolds::Empty), ReplaceVerdict::Pin);
        assert_eq!(
            replace_dead_verdict(SlotHolds::Endpoint { alive: false }),
            ReplaceVerdict::ReplaceDead
        );
    }

    /// A live capability is never overwritten (point 3).
    #[test]
    fn test_reject_replace_dead_over_live_cap() {
        assert_eq!(
            replace_dead_verdict(SlotHolds::Endpoint { alive: true }),
            ReplaceVerdict::Refuse
        );
    }

    #[test]
    fn test_reject_replace_dead_over_a_capability_of_another_kind() {
        assert_eq!(replace_dead_verdict(SlotHolds::Other), ReplaceVerdict::Refuse);
    }
}
