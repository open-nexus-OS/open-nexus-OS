// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: two generations over one region — the arena half of ADR-0065.
//!
//! This module is the RULE, with no allocator, no statics and no `cfg` gate, so
//! the behaviour that matters can be proven on the host. The OS-only
//! `GlobalAlloc` glue in `lib.rs` owns the memory and the lock; it asks this
//! type where the next byte goes.
//!
//! WHY GENERATIONS AT ALL: the service bump never frees, which is right for a
//! service that allocates a bounded working set once. The app-host is not that
//! service — it rebuilds a frame on every structural interaction and throws it
//! away. Measured against the real desktop-shell: 226 560 B per layout call,
//! ~100 KiB per structural emit, and **0 B of live drift over 100 dispatches**.
//! All of it is garbage, so on a never-freeing bump it leaks in full — a
//! 16 MiB heap buys 50–85 interactions, which is why the heap was raised 4 → 8
//! → 16 MiB and why no finite raise is a fix (TASK-0077C).
//!
//! WHY EXACTLY TWO: app-host holds the PREVIOUS frame's boxes and texts to diff
//! against the new ones (`probe/interaction.rs` takes `old_boxes`/`old_texts`
//! before re-laying out). So one generation is live while the next is built,
//! and the one before that is dead. Two is what the consumer needs, not a
//! number that looked round.
//!
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the unit tests below (host)
//! ADR: docs/adr/0065-app-host-frame-phase-allocates-from-a-generation-arena.md

/// Where an allocation should come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// Address inside the open generation.
    Arena(usize),
    /// Not this allocator's business — the caller uses the base heap. Either no
    /// generation is open, or this one is full (see [`Generations::spilled`]).
    BaseHeap,
}

/// Two halves of one region, used alternately.
///
/// Invariant: the half that is NOT open is the retained one and is never
/// written. Opening flips, and resets the half being opened — which is the
/// generation from two frames ago, already dead.
pub(crate) struct Generations {
    /// Base of each half; `0` until [`Generations::init`].
    base: [usize; 2],
    /// Bytes per half.
    half: usize,
    /// Which half is written when a scope is open.
    active: usize,
    /// Bump cursor within the active half, as an offset.
    used: [usize; 2],
    /// Is a scope open? Closed means every allocation is the base heap's.
    open: bool,
    /// Largest `used` ever seen in either half — what the budget probe reports.
    peak: usize,
    /// An allocation did not fit and went to the base heap instead. Sticky: the
    /// point of the arena is that this never happens, so it is reported once
    /// and gated on, not counted.
    spilled: bool,
}

impl Generations {
    pub(crate) const fn empty() -> Self {
        Self {
            base: [0, 0],
            half: 0,
            active: 0,
            used: [0, 0],
            open: false,
            peak: 0,
            spilled: false,
        }
    }

    /// Splits `size` bytes at `base` into two halves. A region too small for
    /// two halves disables the arena rather than half-working.
    pub(crate) fn init(&mut self, base: usize, size: usize) {
        let half = size / 2;
        if half == 0 {
            return;
        }
        self.base = [base, base + half];
        self.half = half;
    }

    /// Whether this arena has memory at all (a service that did not opt in has
    /// none, and every call below is then a no-op).
    pub(crate) fn enabled(&self) -> bool {
        self.half != 0
    }

    /// Starts the next generation: flip, and reset the half being opened.
    ///
    /// That half held the frame from two frames ago. Resetting is ONE store —
    /// no per-object free, which is the whole point.
    pub(crate) fn open(&mut self) {
        if !self.enabled() {
            return;
        }
        self.active ^= 1;
        self.used[self.active] = 0;
        self.open = true;
    }

    /// Ends the scope. The half stays intact — it is the retained generation
    /// the next frame diffs against; only its SUCCESSOR's open resets it.
    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    /// Where the next allocation of `size`/`align` goes.
    pub(crate) fn place(&mut self, size: usize, align: usize) -> Place {
        if !self.open || !self.enabled() {
            return Place::BaseHeap;
        }
        let base = self.base[self.active];
        let cursor = base + self.used[self.active];
        let aligned = (cursor + align.saturating_sub(1)) & !align.saturating_sub(1);
        let Some(end) = aligned.checked_add(size) else {
            self.spilled = true;
            return Place::BaseHeap;
        };
        if end > base + self.half {
            // Deliberately NOT a fatal allocation failure: the app keeps the
            // behaviour it has today (a base-heap allocation) instead of dying
            // mid-frame, and `spilled` makes it loud and gateable. Silent is
            // what the arena exists to end; dead is not an improvement on slow.
            self.spilled = true;
            return Place::BaseHeap;
        }
        self.used[self.active] = end - base;
        if self.used[self.active] > self.peak {
            self.peak = self.used[self.active];
        }
        Place::Arena(aligned)
    }

    /// High-water mark across both halves — the number the budget probe prints.
    pub(crate) fn peak(&self) -> usize {
        self.peak
    }

    /// True once an allocation has not fit. The gate asserts this stays false.
    pub(crate) fn spilled(&self) -> bool {
        self.spilled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A region big enough for two 1 KiB halves, addressed as plain integers —
    /// this type does no memory access, it only says where bytes go.
    fn gens() -> Generations {
        let mut g = Generations::empty();
        g.init(0x1000, 2048);
        g
    }

    /// A service that never opens a scope must be untouched. This is the claim
    /// ADR-0065 makes to justify changing an allocator every service links.
    #[test]
    fn without_a_scope_every_allocation_is_the_base_heap_s() {
        let mut g = gens();
        assert_eq!(g.place(16, 8), Place::BaseHeap);
        assert_eq!(g.place(16, 8), Place::BaseHeap);
        assert_eq!(g.peak(), 0, "the arena cursor must not move");
        assert!(!g.spilled());
    }

    /// And a service that did not opt in has no region at all, so the scope API
    /// degrades to today's behaviour rather than mis-sizing itself.
    #[test]
    fn an_arena_without_memory_is_a_no_op() {
        let mut g = Generations::empty();
        assert!(!g.enabled());
        g.open();
        assert_eq!(g.place(8, 8), Place::BaseHeap);
        assert_eq!(g.peak(), 0);
    }

    /// THE proof: memory is REUSED, not merely "not grown". A reset that
    /// quietly advanced would still report a flat high-water mark on a big
    /// enough region and pass a naive steady-heap test — so the assertion is
    /// on the ADDRESS, which only a real reset can repeat.
    #[test]
    fn the_generation_before_last_is_reused_byte_for_byte() {
        let mut g = gens();
        g.open();
        let first = g.place(64, 8);
        g.close();

        g.open(); // generation 2 — the other half
        let other = g.place(64, 8);
        g.close();
        assert_ne!(first, other, "the retained generation must not be written");

        g.open(); // generation 3 — reuses generation 1's half
        let again = g.place(64, 8);
        g.close();
        assert_eq!(first, again, "a generation must hand back the same bytes");
    }

    /// The retained generation survives its successor's whole frame.
    #[test]
    fn opening_a_generation_does_not_disturb_the_retained_one() {
        let mut g = gens();
        g.open();
        let Place::Arena(retained) = g.place(128, 8) else { panic!("arena expected") };
        g.close();
        g.open();
        for _ in 0..8 {
            let Place::Arena(a) = g.place(64, 8) else { panic!("arena expected") };
            assert!(
                !(retained..retained + 128).contains(&a),
                "generation 2 wrote into the generation it must diff against"
            );
        }
    }

    #[test]
    fn alignment_is_respected_and_the_peak_tracks_the_worst_half() {
        let mut g = gens();
        g.open();
        let Place::Arena(a) = g.place(1, 1) else { panic!("arena expected") };
        let Place::Arena(b) = g.place(8, 64) else { panic!("arena expected") };
        assert_eq!(b % 64, 0, "alignment");
        assert!(b > a);
        let peak_one = g.peak();
        g.close();
        g.open();
        let _ = g.place(512, 8);
        g.close();
        assert!(g.peak() > peak_one, "the peak follows the worst generation, not the last");
    }

    /// An allocation that does not fit SPILLS to the base heap — loudly, via
    /// the sticky flag the gate reads — rather than killing the service
    /// mid-frame. Slow and visible beats dead.
    #[test]
    fn test_reject_an_allocation_larger_than_a_generation() {
        let mut g = gens();
        g.open();
        assert_eq!(g.place(4096, 8), Place::BaseHeap, "4 KiB cannot fit a 1 KiB half");
        assert!(g.spilled(), "a spill must be visible");
        // And the generation is still usable for what does fit.
        assert!(matches!(g.place(32, 8), Place::Arena(_)));
    }

    /// A half that fills mid-generation spills the remainder and keeps what it
    /// already placed — it must not corrupt the cursor.
    #[test]
    fn filling_a_generation_spills_the_rest_without_moving_past_the_half() {
        let mut g = gens();
        g.open();
        let mut placed = 0usize;
        for _ in 0..40 {
            if let Place::Arena(_) = g.place(64, 8) {
                placed += 64;
            }
        }
        assert!(g.spilled(), "40 x 64 B cannot fit a 1 KiB half");
        assert!(placed <= 1024, "placed {placed} B in a 1024 B half");
        assert!(g.peak() <= 1024);
    }
}
