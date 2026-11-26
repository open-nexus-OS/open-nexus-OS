// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: who speaks a dying kernel's last words (TASK-0330 P7) — the pure rule of
//! `trap::fatal`, NOT target-gated so it runs on the host: the first hart to claim the word
//! speaks, the same hart claiming again is inside its own last words (a fault within them), and
//! every other hart is silenced from then on.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::one_hart_speaks_and_every_other_is_silenced`,
//!   `tests::test_reject_a_second_speaker`)
//! INVARIANTS: the word, once claimed, is never released — a kernel dies once.

#[cfg(any(test, target_os = "none"))]
use core::sync::atomic::{AtomicUsize, Ordering};

/// What a hart's claim to the last words came to.
#[cfg(any(test, target_os = "none"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Claim {
    /// This hart speaks: the console is its own from here on.
    First,
    /// This hart already speaks — a fault or a panic inside its own last words.
    Nested,
    /// Another hart speaks: this one says nothing.
    Silenced,
}

/// Claims the word in `slot` (0 while the kernel lives, else the speaker as `cpu + 1`) for `cpu`.
#[cfg(any(test, target_os = "none"))]
pub(crate) fn claim(slot: &AtomicUsize, cpu: usize) -> Claim {
    match slot.compare_exchange(0, cpu + 1, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => Claim::First,
        Err(owner) if owner == cpu + 1 => Claim::Nested,
        Err(_) => Claim::Silenced,
    }
}

/// Whether `cpu` must say nothing more: another hart holds the word in `slot`.
#[cfg(any(test, target_os = "none"))]
#[must_use]
pub(crate) fn silenced(slot: &AtomicUsize, cpu: usize) -> bool {
    let owner = slot.load(Ordering::Acquire);
    owner != 0 && owner != cpu + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_hart_speaks_and_every_other_is_silenced() {
        let slot = AtomicUsize::new(0);
        assert!(!silenced(&slot, 0) && !silenced(&slot, 3));
        assert_eq!(claim(&slot, 2), Claim::First);
        assert_eq!(claim(&slot, 2), Claim::Nested);
        assert!(!silenced(&slot, 2));
        assert!(silenced(&slot, 0) && silenced(&slot, 5));
    }

    #[test]
    fn test_reject_a_second_speaker() {
        let slot = AtomicUsize::new(0);
        assert_eq!(claim(&slot, 0), Claim::First);
        for cpu in 1..8 {
            assert_eq!(claim(&slot, cpu), Claim::Silenced);
        }
        // The word stays with the first: a later claim by it is still its own.
        assert_eq!(claim(&slot, 0), Claim::Nested);
    }
}
