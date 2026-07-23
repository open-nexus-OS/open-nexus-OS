// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the VMO-arena ranges backing ONE task's process image — the
//! PT_LOAD segments, the stack, and the bootstrap metadata pages `exec`
//! allocates for a child. `exec` records them here; task teardown hands them
//! back to the arena. Without this record the arena was bump-only for
//! process images: every launch consumed its image permanently, so a session
//! that opened and closed a handful of apps exhausted it (RFC-0075 8e).
//! NOT target-gated (pure usize logic) so the tests below run on host —
//! same rationale as `waitset`/`fence`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (capacity, drain, overflow accounting)
//! INVARIANTS: fixed capacity (no kernel-heap churn per process); an
//!   overflowing record COUNTS what it could not track — never silently
//!   forgets that memory exists.

extern crate alloc;

/// Ranges recorded per task. A process image is a handful of PT_LOAD
/// segments (3 in the shipped services) plus stack, metadata and info pages;
/// 12 leaves headroom without putting per-process churn on the kernel heap.
const MAX_RANGES: usize = 12;

/// The arena ranges owned by one task's process image.
#[derive(Clone, Copy)]
pub struct ImageAllocs {
    ranges: [(usize, usize); MAX_RANGES],
    len: usize,
    /// Ranges that did not fit. Their memory stays allocated (honest leak,
    /// reported at teardown) rather than being freed without a record.
    untracked: usize,
}

impl Default for ImageAllocs {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageAllocs {
    /// An empty record.
    #[must_use]
    pub const fn new() -> Self {
        Self { ranges: [(0, 0); MAX_RANGES], len: 0, untracked: 0 }
    }

    /// Records one arena range. Zero-length ranges are ignored; overflow is
    /// counted, never dropped silently.
    pub fn push(&mut self, base: usize, len: usize) {
        if len == 0 {
            return;
        }
        if self.len == MAX_RANGES {
            self.untracked += 1;
            return;
        }
        self.ranges[self.len] = (base, len);
        self.len += 1;
    }

    /// Empties the record and returns what it held — the teardown handoff
    /// (the task must never hold ranges it no longer owns).
    #[must_use]
    pub fn take(&mut self) -> Self {
        core::mem::replace(self, Self::new())
    }

    /// The recorded ranges, in allocation order.
    pub fn iter(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.ranges[..self.len].iter().copied()
    }

    /// Number of recorded ranges.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether nothing is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Ranges that exceeded the capacity and stay allocated.
    #[must_use]
    pub fn untracked(&self) -> usize {
        self.untracked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_drains_in_order() {
        let mut a = ImageAllocs::new();
        assert!(a.is_empty());
        a.push(0x1000, 0x2000);
        a.push(0x8000, 0x1000);
        assert_eq!(a.len(), 2);
        let drained = a.take();
        assert!(a.is_empty(), "take must leave the task owning nothing");
        let got: alloc::vec::Vec<_> = drained.iter().collect();
        assert_eq!(got, alloc::vec![(0x1000, 0x2000), (0x8000, 0x1000)]);
    }

    #[test]
    fn zero_length_ranges_are_not_recorded() {
        let mut a = ImageAllocs::new();
        a.push(0x1000, 0);
        assert!(a.is_empty());
        assert_eq!(a.untracked(), 0);
    }

    #[test]
    fn overflow_is_counted_not_dropped_silently() {
        let mut a = ImageAllocs::new();
        for i in 0..MAX_RANGES + 3 {
            a.push(0x1000 * (i + 1), 0x1000);
        }
        assert_eq!(a.len(), MAX_RANGES);
        assert_eq!(a.untracked(), 3, "over-capacity ranges must be reported");
    }
}
