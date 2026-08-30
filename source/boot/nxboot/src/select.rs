// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Slot selection over the decoded BSB (RFC-0089 §7 step 2;
//! ADR-0058 actuator discipline). Pure and host-tested: given the current
//! selection block, decide which slot to boot and which BSB state (if any)
//! must be persisted BEFORE the image is loaded, so a boot loop into a
//! broken image converges instead of retrying forever.
//!
//! Actuator discipline (ADR-0058): the loader's write mutates ONLY
//! `tries_left` (trial decrement) or clears `next_slot` (exhaustion) — the
//! standing `active_slot` is bootctld's to change on health commit; during
//! a trial it keeps naming the last known-good slot, which is exactly what
//! the exhaustion fallback boots.
//! OWNERS: @security @runtime
//! STATUS: Experimental (TASK-0289 Phase A)
//! TEST_COVERAGE: full state table below
//! ADR: docs/adr/0058-boot-selection-block-dual-actor-discipline.md

use bootfmt::bsb::{Bsb, Slot};

/// Why the plan picked its slot (drives the normative uart markers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanKind {
    /// No trial pending: boot the standing active slot, no BSB write.
    Active,
    /// Trial boot of `next_slot`; the decrement is persisted before load
    /// (`nxboot: tries <n>-><n-1> (slot=<s> trial)`).
    Trial {
        /// `tries_left` value being persisted (after the decrement).
        tries_after: u8,
    },
    /// `next_slot` was set but its tries are exhausted: clear it and boot
    /// the standing active slot
    /// (`nxboot: fallback (slot=<s> exhausted) -> slot=<s'>`).
    ExhaustedFallback {
        /// The slot whose trial budget ran out.
        attempted: Slot,
    },
}

/// One boot's selection decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Slot to load and verify first (verify failure falls back to
    /// `boot.other()` per RFC-0089 §7 step 3 — no BSB write on that path).
    pub boot: Slot,
    /// BSB state to persist BEFORE loading, if the plan mutates state.
    /// The caller writes it to the ALTERNATE block (power-cut atomic).
    pub write: Option<Bsb>,
    pub kind: PlanKind,
}

/// Decides the boot plan for the current selection block.
pub fn plan(cur: &Bsb) -> Plan {
    match cur.next_slot {
        Some(next) if cur.tries_left > 0 => {
            let mut w = *cur;
            w.seq = cur.seq.saturating_add(1);
            w.tries_left = cur.tries_left - 1;
            Plan { boot: next, write: Some(w), kind: PlanKind::Trial { tries_after: w.tries_left } }
        }
        Some(next) => {
            let mut w = *cur;
            w.seq = cur.seq.saturating_add(1);
            w.next_slot = None;
            Plan {
                boot: cur.active_slot,
                write: Some(w),
                kind: PlanKind::ExhaustedFallback { attempted: next },
            }
        }
        None => Plan { boot: cur.active_slot, write: None, kind: PlanKind::Active },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bsb(active: Slot, next: Option<Slot>, tries: u8) -> Bsb {
        Bsb {
            seq: 10,
            active_slot: active,
            next_slot: next,
            tries_left: tries,
            health_committed: true,
            boot_target: 0,
            rollback_min_index: 5,
        }
    }

    #[test]
    fn steady_state_boots_active_without_write() {
        for active in [Slot::A, Slot::B] {
            let p = plan(&bsb(active, None, 0));
            assert_eq!(p.boot, active);
            assert_eq!(p.write, None);
            assert_eq!(p.kind, PlanKind::Active);
        }
    }

    #[test]
    fn trial_decrements_before_load_and_touches_only_tries() {
        let cur = bsb(Slot::A, Some(Slot::B), 2);
        let p = plan(&cur);
        assert_eq!(p.boot, Slot::B);
        assert_eq!(p.kind, PlanKind::Trial { tries_after: 1 });
        let w = p.write.expect("trial persists the decrement");
        assert_eq!(w.tries_left, 1);
        assert_eq!(w.seq, 11, "alternate-block write bumps seq");
        // ADR-0058: everything else is untouched.
        assert_eq!(
            Bsb { tries_left: cur.tries_left, seq: cur.seq, ..w },
            cur,
            "trial write mutates only tries_left (+seq)"
        );
    }

    #[test]
    fn last_try_still_boots_the_trial_slot() {
        let p = plan(&bsb(Slot::A, Some(Slot::B), 1));
        assert_eq!(p.boot, Slot::B);
        assert_eq!(p.kind, PlanKind::Trial { tries_after: 0 });
        assert_eq!(p.write.expect("write").tries_left, 0);
    }

    #[test]
    fn exhaustion_clears_next_and_boots_active() {
        let cur = bsb(Slot::A, Some(Slot::B), 0);
        let p = plan(&cur);
        assert_eq!(p.boot, Slot::A, "fallback boots the standing active slot");
        assert_eq!(p.kind, PlanKind::ExhaustedFallback { attempted: Slot::B });
        let w = p.write.expect("exhaustion persists the cleared trial");
        assert_eq!(w.next_slot, None);
        assert_eq!(w.seq, 11);
        assert_eq!(
            Bsb { next_slot: cur.next_slot, seq: cur.seq, ..w },
            cur,
            "exhaustion write mutates only next_slot (+seq)"
        );
    }

    #[test]
    fn seq_saturates_instead_of_wrapping() {
        let mut cur = bsb(Slot::A, Some(Slot::B), 2);
        cur.seq = u64::MAX;
        let w = plan(&cur).write.expect("write");
        assert_eq!(w.seq, u64::MAX, "monotonic claim never wraps to 0");
    }

    #[test]
    fn next_equal_active_is_mechanically_a_trial() {
        // bootctld never schedules this, but the loader stays total.
        let p = plan(&bsb(Slot::A, Some(Slot::A), 2));
        assert_eq!(p.boot, Slot::A);
        assert_eq!(p.kind, PlanKind::Trial { tries_after: 1 });
    }
}
