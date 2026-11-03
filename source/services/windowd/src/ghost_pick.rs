// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pure half of windowd's ghosts (RFC-0112 Phase 3, TASK-0348 P9). After a
//! compositor restart every window of the compositor before is a GHOST: a slot showing the app's
//! last pixels where they were. An owner's rejoin (its create) takes its ghost over — the one
//! of the surface's size first, the topmost of those — so the window stays where it was and the
//! pixels never blink; execd's list of the apps it told closes the ghosts of every other owner.
//! Identity is the kernel's sender id captured at the create (`owner`), never a payload string.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below incl. `test_reject_*`

/// A ghost slot as the pick sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ghost {
    pub slot: usize,
    /// The owner's kernel id.
    pub owner: u64,
    /// The ghost's frame.
    pub size: (u32, u32),
    /// Its place in the stack (higher = nearer the viewer).
    pub z: i16,
}

/// The ghost `owner`'s new surface of `size` takes over: one of the owner's own ghosts only — of
/// the same size when there is one, the topmost of the candidates. `None`: no ghost of the owner.
#[must_use]
pub(crate) fn pick(ghosts: &[Ghost], owner: u64, size: (u16, u16)) -> Option<usize> {
    if owner == 0 {
        return None;
    }
    let size = (u32::from(size.0), u32::from(size.1));
    ghosts.iter().filter(|g| g.owner == owner).max_by_key(|g| (g.size == size, g.z)).map(|g| g.slot)
}

/// Whether a ghost of `owner` closes now: its owner is none of the apps execd told.
#[must_use]
pub(crate) fn is_gone(owner: u64, alive: &[u64]) -> bool {
    !alive.contains(&owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u64 = 0xA11CE;
    const B: u64 = 0xB0B;

    fn ghosts() -> [Ghost; 4] {
        [
            Ghost { slot: 0, owner: A, size: (640, 480), z: 1 },
            Ghost { slot: 1, owner: B, size: (640, 480), z: 2 },
            Ghost { slot: 2, owner: A, size: (320, 240), z: 3 },
            Ghost { slot: 3, owner: A, size: (640, 480), z: 4 },
        ]
    }

    #[test]
    fn a_rejoin_takes_the_owners_ghost_of_its_size_the_topmost_first() {
        assert_eq!(pick(&ghosts(), A, (640, 480)), Some(3), "the topmost of two alike");
        assert_eq!(pick(&ghosts(), A, (320, 240)), Some(2), "the one of its size");
        assert_eq!(pick(&ghosts(), A, (800, 600)), Some(3), "no size alike: the topmost");
        assert_eq!(pick(&ghosts(), B, (320, 240)), Some(1), "another owner's only ghost");
    }

    #[test]
    fn test_reject_another_owners_ghost_or_an_unnamed_owner() {
        assert_eq!(pick(&ghosts(), 0xC0FFEE, (640, 480)), None, "no ghost of this owner");
        assert_eq!(pick(&ghosts(), 0, (640, 480)), None, "an unnamed spawn owns nothing");
        assert_eq!(pick(&[], A, (640, 480)), None);
    }

    #[test]
    fn a_ghost_closes_when_execd_names_its_owner_gone() {
        assert!(!is_gone(A, &[A, B]));
        assert!(is_gone(0xC0FFEE, &[A, B]));
        assert!(is_gone(A, &[]), "nobody told: every ghost goes");
    }
}
