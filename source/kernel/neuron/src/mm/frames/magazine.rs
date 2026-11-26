// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a hart's magazine of single frames (TASK-0330 P3): the frames one refill took from
//! the pool, handed out lowest first, so a hart that takes a frame per page fault asks the pool
//! once in [`BATCH`] faults instead of every time. Only allocations pass through it — a freed
//! frame goes straight back to the pool, whose refusals (a double free, a foreign block, a
//! hole) therefore stay exact; every frame in a magazine is one the pool handed out and counts
//! as allocated there. A pool that runs dry takes the magazines' frames back (`flush`) before
//! it refuses anyone. Pure index logic, so it is proven on the host.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below (host): the order of a refill, a refill of a magazine that is
//!   not empty, a flush, the counters; in the kernel `KINIT: mm magazines (…)`
//! INVARIANTS: a frame is in at most one magazine; `take` never invents a frame; a refill is
//!   accepted only by an empty magazine and never beyond `BATCH`

/// Frames one refill takes from the pool.
pub const BATCH: usize = 16;

/// One hart's frames, lowest first.
pub struct Magazine {
    bases: [u64; BATCH],
    next: usize,
    len: usize,
    served: u64,
    refills: u64,
}

impl Magazine {
    /// An empty magazine.
    #[must_use]
    pub const fn new() -> Self {
        Self { bases: [0; BATCH], next: 0, len: 0, served: 0, refills: 0 }
    }

    /// Frames held.
    #[must_use]
    pub fn held(&self) -> usize {
        self.len - self.next
    }

    /// The next frame's base, in the order the refill named them.
    pub fn take(&mut self) -> Option<u64> {
        let base = *self.bases[..self.len].get(self.next)?;
        self.next += 1;
        self.served += 1;
        Some(base)
    }

    /// Takes `frames` as the new content and answers how many it took: all of them (up to
    /// `BATCH`) when it is empty, none otherwise — the caller gives the rest back to the pool.
    pub fn refill(&mut self, frames: &[u64]) -> usize {
        if self.held() != 0 || frames.is_empty() {
            return 0;
        }
        let taken = frames.len().min(BATCH);
        self.bases[..taken].copy_from_slice(&frames[..taken]);
        (self.next, self.len) = (0, taken);
        self.refills += 1;
        taken
    }

    /// Every frame held into `out`, lowest first; the magazine is empty afterwards. Answers how
    /// many.
    pub fn flush(&mut self, out: &mut [u64; BATCH]) -> usize {
        let held = self.held();
        out[..held].copy_from_slice(&self.bases[self.next..self.len]);
        (self.next, self.len) = (0, 0);
        held
    }

    /// `(frames handed out, refills)` since boot.
    #[must_use]
    pub fn counts(&self) -> (u64, u64) {
        (self.served, self.refills)
    }
}

impl Default for Magazine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refill_is_handed_out_in_its_order() {
        let mut magazine = Magazine::new();
        assert_eq!(magazine.take(), None);
        assert_eq!(magazine.refill(&[0x3000, 0x1000, 0x2000]), 3);
        assert_eq!(magazine.held(), 3);
        assert_eq!(magazine.take(), Some(0x3000));
        assert_eq!(magazine.take(), Some(0x1000));
        assert_eq!(magazine.take(), Some(0x2000));
        assert_eq!(magazine.take(), None);
        assert_eq!(magazine.counts(), (3, 1));
    }

    #[test]
    fn test_reject_refill_of_a_magazine_that_holds_frames() {
        let mut magazine = Magazine::new();
        assert_eq!(magazine.refill(&[0x1000, 0x2000]), 2);
        assert_eq!(magazine.take(), Some(0x1000));
        // One frame is still held: a second refill would lose it.
        assert_eq!(magazine.refill(&[0x9000]), 0);
        assert_eq!(magazine.take(), Some(0x2000));
        assert_eq!(magazine.refill(&[]), 0);
        assert_eq!(magazine.counts(), (2, 1));
    }

    #[test]
    fn test_reject_refill_beyond_a_batch() {
        let mut magazine = Magazine::new();
        let frames: [u64; BATCH + 4] = core::array::from_fn(|i| 0x1000 * (i as u64 + 1));
        assert_eq!(magazine.refill(&frames), BATCH);
        assert_eq!(magazine.held(), BATCH);
    }

    #[test]
    fn a_flush_names_what_is_left_and_empties() {
        let mut magazine = Magazine::new();
        magazine.refill(&[0x1000, 0x2000, 0x3000]);
        assert_eq!(magazine.take(), Some(0x1000));
        let mut out = [0u64; BATCH];
        assert_eq!(magazine.flush(&mut out), 2);
        assert_eq!(&out[..2], &[0x2000, 0x3000]);
        assert_eq!(magazine.held(), 0);
        assert_eq!(magazine.take(), None);
        assert_eq!(magazine.flush(&mut out), 0);
        // Empty again: the next refill is accepted.
        assert_eq!(magazine.refill(&[0x4000]), 1);
    }
}
