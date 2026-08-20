// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The virtio-upgrade window as a pure, host-tested state machine
//! (TASK-0049 / RFC-0087 "exhaustion is an event"). statefsd opens on RAM to
//! reach `ready` deterministically and upgrades to virtio-blk only while
//! still pristine — before this module, BOTH ways of losing that window were
//! silent: the first mutating op arriving early closed it wordlessly, and
//! the retry budget ran out without a final verdict. Durability silently
//! became RAM-only for the whole boot. The serve loop now feeds this machine
//! and announces every terminal degradation exactly once; the QEMU harness
//! treats a degrade marker in a proof boot as fatal (the proof profiles MUST
//! upgrade).
//!
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests below (deterministic transition table)
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§1 exhaustion)

/// How many times the serve loop retries `JournalEngine::open` on virtio
/// before giving up for the boot (unchanged budget from the pre-machine code).
pub const VIRTIO_MAX_RETRIES: u8 = 5;

/// Where the upgrade window stands. Terminal states are permanent for the
/// boot — their announcement fires on ENTRY, exactly once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeState {
    /// Still RAM-backed and pristine: an upgrade may still happen.
    Waiting {
        /// Failed `open` attempts so far (gives up at [`VIRTIO_MAX_RETRIES`]).
        retries: u8,
    },
    /// Virtio backend is live; durability is real.
    Upgraded,
    /// A mutating op closed the window before the device grant arrived —
    /// RAM-backed for the rest of the boot (RFC-0087 degradation).
    DegradedMissedWindow,
    /// The retry budget is exhausted — RAM-backed for the rest of the boot.
    DegradedRetriesExhausted,
}

/// What the serve loop must do after feeding an event (marker emission stays
/// in the OS layer; the machine only decides).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeAction {
    /// Nothing to announce.
    None,
    /// Entered [`UpgradeState::DegradedMissedWindow`] — announce once.
    AnnounceMissedWindow,
    /// Entered [`UpgradeState::DegradedRetriesExhausted`] — announce once.
    AnnounceRetriesExhausted,
}

impl UpgradeState {
    /// Fresh boot: waiting with a full retry budget.
    pub fn new() -> Self {
        Self::Waiting { retries: 0 }
    }

    /// `true` while the serve loop should still probe for the device grant.
    pub fn wants_upgrade(&self) -> bool {
        matches!(self, Self::Waiting { .. })
    }

    /// `JournalEngine::open(Virtio)` succeeded.
    pub fn on_open_ok(&mut self) -> UpgradeAction {
        if self.wants_upgrade() {
            *self = Self::Upgraded;
        }
        UpgradeAction::None
    }

    /// `JournalEngine::open(Virtio)` failed; burns one retry. Exhausting the
    /// budget is a terminal degradation (announced once).
    pub fn on_open_failed(&mut self) -> UpgradeAction {
        if let Self::Waiting { retries } = *self {
            let next = retries.saturating_add(1);
            if next >= VIRTIO_MAX_RETRIES {
                *self = Self::DegradedRetriesExhausted;
                return UpgradeAction::AnnounceRetriesExhausted;
            }
            *self = Self::Waiting { retries: next };
        }
        UpgradeAction::None
    }

    /// A mutating op (PUT/DEL/SYNC/REOPEN/txn) was accepted. After the
    /// upgrade this is routine; before it, the window is gone for the boot —
    /// a terminal degradation (announced once).
    pub fn on_mutating_op(&mut self) -> UpgradeAction {
        if self.wants_upgrade() {
            *self = Self::DegradedMissedWindow;
            return UpgradeAction::AnnounceMissedWindow;
        }
        UpgradeAction::None
    }
}

impl Default for UpgradeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_upgrades_and_mutations_stay_silent() {
        let mut w = UpgradeState::new();
        assert!(w.wants_upgrade());
        assert_eq!(w.on_open_ok(), UpgradeAction::None);
        assert_eq!(w, UpgradeState::Upgraded);
        // Post-upgrade mutations are routine — never a degradation.
        assert_eq!(w.on_mutating_op(), UpgradeAction::None);
        assert_eq!(w, UpgradeState::Upgraded);
        assert!(!w.wants_upgrade());
    }

    #[test]
    fn early_mutating_op_is_a_terminal_announced_degradation() {
        let mut w = UpgradeState::new();
        assert_eq!(w.on_mutating_op(), UpgradeAction::AnnounceMissedWindow);
        assert_eq!(w, UpgradeState::DegradedMissedWindow);
        // Announced exactly once; further mutations and events stay silent
        // and can never resurrect the window.
        assert_eq!(w.on_mutating_op(), UpgradeAction::None);
        assert_eq!(w.on_open_ok(), UpgradeAction::None);
        assert_eq!(w, UpgradeState::DegradedMissedWindow);
        assert!(!w.wants_upgrade());
    }

    #[test]
    fn retry_budget_exhaustion_is_a_terminal_announced_degradation() {
        let mut w = UpgradeState::new();
        for _ in 0..(VIRTIO_MAX_RETRIES - 1) {
            assert_eq!(w.on_open_failed(), UpgradeAction::None);
            assert!(w.wants_upgrade(), "still retrying inside the budget");
        }
        assert_eq!(w.on_open_failed(), UpgradeAction::AnnounceRetriesExhausted);
        assert_eq!(w, UpgradeState::DegradedRetriesExhausted);
        // Announced exactly once; terminal.
        assert_eq!(w.on_open_failed(), UpgradeAction::None);
        assert_eq!(w.on_open_ok(), UpgradeAction::None);
        assert!(!w.wants_upgrade());
    }

    #[test]
    fn success_on_the_last_retry_still_upgrades() {
        let mut w = UpgradeState::new();
        for _ in 0..(VIRTIO_MAX_RETRIES - 1) {
            let _ = w.on_open_failed();
        }
        assert_eq!(w.on_open_ok(), UpgradeAction::None);
        assert_eq!(w, UpgradeState::Upgraded);
    }
}
