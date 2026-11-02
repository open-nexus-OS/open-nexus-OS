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

extern crate alloc;
use alloc::vec::Vec;

/// The desktop channel's tracking index (the nonce-bound channels are `0..`, as many as bind —
/// RFC-0114 §7: no channel table cap).
pub(crate) const DESKTOP: usize = usize::MAX;
/// Consecutive refused sends before a channel stops being retried (the
/// watch-table reclaim precedent). A rebind revives it.
const DEAD_AFTER_FAILURES: u8 = 8;

/// One delivery target's state: the generation it last took, its refusals in a row.
#[derive(Clone, Copy, Debug, Default)]
struct Track {
    pushed_gen: u32,
    failures: u8,
}

pub(crate) struct PresentationState {
    /// Wrapping generation of the CURRENT snapshot (bumped on every change).
    gen: u32,
    /// Per nonce-bound channel (its index in the runtime's table), grown as channels bind.
    channels: Vec<Track>,
    desktop: Track,
}

impl PresentationState {
    pub(crate) const fn new() -> Self {
        // gen starts at 1 so a fresh channel (pushed_gen 0) is immediately due.
        Self { gen: 1, channels: Vec::new(), desktop: Track { pushed_gen: 0, failures: 0 } }
    }

    fn track(&self, idx: usize) -> Option<&Track> {
        if idx == DESKTOP {
            return Some(&self.desktop);
        }
        self.channels.get(idx)
    }

    fn track_mut(&mut self, idx: usize) -> Option<&mut Track> {
        if idx == DESKTOP {
            return Some(&mut self.desktop);
        }
        self.channels.get_mut(idx)
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
        if idx != DESKTOP && idx >= self.channels.len() {
            if self.channels.try_reserve(idx + 1 - self.channels.len()).is_err() {
                return;
            }
            self.channels.resize(idx + 1, Track::default());
        }
        let owed = self.gen.wrapping_sub(1);
        if let Some(t) = self.track_mut(idx) {
            *t = Track { pushed_gen: owed, failures: 0 };
        }
    }

    /// The channel at `idx` left the table: the ones after it move down with it.
    pub(crate) fn forget(&mut self, idx: usize) {
        if idx < self.channels.len() {
            self.channels.remove(idx);
        }
    }

    /// Whether `idx` owes a push of the current snapshot.
    pub(crate) fn due(&self, idx: usize) -> bool {
        self.track(idx)
            .is_some_and(|t| t.failures < DEAD_AFTER_FAILURES && t.pushed_gen != self.gen)
    }

    /// Records one NONBLOCK send attempt's outcome for `idx`.
    pub(crate) fn note_send(&mut self, idx: usize, ok: bool) {
        let gen = self.gen;
        let Some(t) = self.track_mut(idx) else { return };
        if ok {
            *t = Track { pushed_gen: gen, failures: 0 };
        } else {
            t.failures = t.failures.saturating_add(1);
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
    fn desktop_slot_is_tracked_and_an_unbound_channel_is_inert() {
        let mut p = PresentationState::new();
        p.note_bound(DESKTOP);
        assert!(p.due(DESKTOP));
        p.note_send(DESKTOP, true);
        assert!(!p.due(DESKTOP));
        // A channel never bound is never due, and a send to it changes nothing.
        p.note_send(40, true);
        assert!(!p.due(40));
    }

    /// RFC-0114 §7: as many channels as bind are tracked; one leaving the table takes its state
    /// with it and the ones after it keep theirs.
    #[test]
    fn channels_grow_and_a_forgotten_one_shifts_the_rest() {
        let mut p = PresentationState::new();
        for i in 0..20 {
            p.note_bound(i);
        }
        p.note_send(19, true);
        assert!(!p.due(19) && p.due(18));
        p.forget(3);
        assert!(!p.due(18), "the delivered channel moved down with the table");
        assert!(p.due(17));
        assert!(!p.due(19), "past the table: nothing");
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
