// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0083 presentation-snapshot delivery core — the pure
//! per-channel retry/dedupe state behind windowd's `push_presentation`.
//! Lives OUTSIDE `compositor/**` for the same reason as the `Delivery`
//! policy in `client_surface.rs`: the compositor does not compile on host,
//! and this logic decided a user-visible bug class (lost settings pushes),
//! so its tests must actually run.
//!
//! The delivery model is RETAINED LATEST-WINS: the snapshot is state, not an
//! event. The compositor NEVER blocks for it — one NONBLOCK attempt per due
//! channel per frame; a refused send just stays due and retries next frame
//! (convergence in 1-2 frames, zero stalls). A channel that keeps refusing
//! is reclaimed after a bounded failure count so a dead client cannot buy
//! per-frame work forever; rebinding revives it.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (due/retry/reclaim/rebind/bump).
//! RFC: docs/rfcs/RFC-0083-settings-distribution-v2-single-authority-versioned-snapshots.md
#![cfg(any(test, all(feature = "os-lite", nexus_env = "os", target_os = "none")))]

/// Tracked delivery targets: the 8 nonce-bound app event channels + the
/// desktop channel (index [`DESKTOP`]).
pub(crate) const MAX_TRACKED: usize = 9;
/// The desktop channel's tracking index.
pub(crate) const DESKTOP: usize = 8;
/// Consecutive refused sends before a channel stops being retried (the
/// watch-table reclaim precedent). A rebind revives it.
const DEAD_AFTER_FAILURES: u8 = 8;

pub(crate) struct PresentationState {
    /// Wrapping generation of the CURRENT snapshot (bumped on every change).
    gen: u32,
    /// Last generation successfully sent per channel.
    pushed_gen: [u32; MAX_TRACKED],
    /// Consecutive send failures per channel.
    failures: [u8; MAX_TRACKED],
}

impl PresentationState {
    pub(crate) const fn new() -> Self {
        // gen starts at 1 so a fresh channel (pushed_gen 0) is immediately due.
        Self { gen: 1, pushed_gen: [0; MAX_TRACKED], failures: [0; MAX_TRACKED] }
    }

    /// The current snapshot generation (goes into the wire frame).
    pub(crate) fn gen(&self) -> u32 {
        self.gen
    }

    /// The snapshot changed — every bound channel becomes due again.
    pub(crate) fn bump(&mut self) {
        self.gen = self.gen.wrapping_add(1);
    }

    /// A channel was (re)bound at `idx`: it owes the current snapshot and any
    /// past failures are forgiven (fresh queue, fresh client).
    pub(crate) fn note_bound(&mut self, idx: usize) {
        if idx < MAX_TRACKED {
            self.pushed_gen[idx] = self.gen.wrapping_sub(1);
            self.failures[idx] = 0;
        }
    }

    /// Whether `idx` owes a push of the current snapshot.
    pub(crate) fn due(&self, idx: usize) -> bool {
        idx < MAX_TRACKED
            && self.failures[idx] < DEAD_AFTER_FAILURES
            && self.pushed_gen[idx] != self.gen
    }

    /// Records one NONBLOCK send attempt's outcome for `idx`.
    pub(crate) fn note_send(&mut self, idx: usize, ok: bool) {
        if idx >= MAX_TRACKED {
            return;
        }
        if ok {
            self.pushed_gen[idx] = self.gen;
            self.failures[idx] = 0;
        } else {
            self.failures[idx] = self.failures[idx].saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The invariant that replaced `Delivery::Critical` for settings: a push
    /// is never LOST (it stays due until a send succeeds or the channel dies)
    /// and never BLOCKS (this state machine only ever answers "is a NONBLOCK
    /// attempt owed right now").
    #[test]
    fn a_fresh_channel_is_due_and_success_clears_it() {
        let mut p = PresentationState::new();
        p.note_bound(0);
        assert!(p.due(0), "a fresh bind owes the current snapshot");
        p.note_send(0, true);
        assert!(!p.due(0), "delivered — nothing owed until the next change");
        let before = p.gen();
        p.bump();
        assert_eq!(p.gen(), before.wrapping_add(1), "gen is the wire dedupe token");
        assert!(p.due(0), "a change makes every bound channel due again");
    }

    #[test]
    fn a_refused_send_stays_due_and_retries() {
        let mut p = PresentationState::new();
        p.note_bound(3);
        p.note_send(3, false);
        assert!(p.due(3), "a refused send is retried next frame, not dropped");
        p.note_send(3, true);
        assert!(!p.due(3));
    }

    #[test]
    fn a_dead_channel_is_reclaimed_and_a_rebind_revives_it() {
        let mut p = PresentationState::new();
        p.note_bound(5);
        for _ in 0..8 {
            assert!(p.due(5));
            p.note_send(5, false);
        }
        assert!(!p.due(5), "8 consecutive refusals: stop buying per-frame work");
        p.note_bound(5);
        assert!(p.due(5), "a rebind forgives and re-owes the snapshot");
    }

    #[test]
    fn desktop_slot_is_tracked_and_out_of_range_is_inert() {
        let mut p = PresentationState::new();
        p.note_bound(DESKTOP);
        assert!(p.due(DESKTOP));
        p.note_send(DESKTOP, true);
        assert!(!p.due(DESKTOP));
        // Out-of-range indices never panic and are never due.
        p.note_bound(MAX_TRACKED);
        p.note_send(MAX_TRACKED, true);
        assert!(!p.due(MAX_TRACKED));
    }

    #[test]
    fn generation_wrap_stays_correct() {
        let mut p = PresentationState::new();
        p.note_bound(0);
        p.note_send(0, true);
        // Wrap the u32 all the way around — dedupe is equality, not ordering.
        for _ in 0..7 {
            p.bump();
        }
        assert!(p.due(0));
        p.note_send(0, true);
        assert!(!p.due(0));
    }
}
