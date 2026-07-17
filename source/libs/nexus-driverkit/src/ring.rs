// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Bounded submit ring — per-slot lifecycle + backpressure + completion counting.
//!
//! This is the device-agnostic generalisation of gpud's `CtrlQueue` (ADR-0032): a fixed set
//! of in-flight slots tracked by a `u32` busy bitmask, allocated round-robin, freed on
//! completion. It is pure bookkeeping — the device server owns the actual command encoding,
//! doorbell, and harvest, calling [`SubmitRing::try_alloc`] before submitting and
//! [`SubmitRing::complete`] when the device reports a slot done.
//!
//! INVARIANTS:
//! - Bounded: at most [`MAX_SLOTS`] (32) slots; `try_alloc` returns `None` when full
//!   (backpressure — the caller waits on completion, e.g. a GPU IRQ or `fence_wait`, then
//!   retries). No unbounded growth, no allocation.
//! - Monotonic tickets: each `try_alloc` hands out a strictly increasing [`Ticket`]; the
//!   `completed` counter rises by one per `complete`. Completions may be out of order
//!   (slot 3 before slot 0) — the ring tracks slot freedom, not ordering. In-order
//!   completion semantics, when needed, come from the kernel timeline fence.

/// Maximum number of in-flight slots a ring can have (bounded by the `u32` busy bitmask).
pub const MAX_SLOTS: usize = 32;

/// A ring slot index (`0..capacity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Slot(pub u8);

/// A monotonically increasing submission ticket. A completion fence is signalled to the
/// matching count so consumers can `fence_wait(target)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ticket(pub u64);

/// Ring operation errors (deterministic; no panics on misuse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingError {
    /// Slot index is outside `0..capacity`.
    InvalidSlot,
    /// Completing a slot that is not currently in flight (double-complete / stale slot).
    NotInFlight,
}

/// A bounded, allocation-free in-flight submit ring.
#[derive(Debug, Clone)]
pub struct SubmitRing {
    /// Number of usable slots, `1..=MAX_SLOTS`.
    slots: u8,
    /// Bitmask of slots currently in flight (bit `i` set ⇒ slot `i` busy).
    busy: u32,
    /// Round-robin allocation cursor.
    next: u8,
    /// Ticket assigned to each in-flight slot (valid only where `busy` bit is set).
    ticket_of: [u64; MAX_SLOTS],
    /// Next ticket to hand out (also the total number of submissions).
    submitted: u64,
    /// Total completions — the value a completion fence is signalled to.
    completed: u64,
}

impl SubmitRing {
    /// Create a ring with `slots` in-flight slots. `slots` is clamped to `1..=MAX_SLOTS`.
    pub fn new(slots: usize) -> Self {
        let slots = slots.clamp(1, MAX_SLOTS) as u8;
        Self { slots, busy: 0, next: 0, ticket_of: [0; MAX_SLOTS], submitted: 0, completed: 0 }
    }

    /// Number of in-flight slots the ring supports.
    pub fn capacity(&self) -> usize {
        self.slots as usize
    }

    /// How many slots are currently in flight.
    pub fn in_flight(&self) -> usize {
        self.busy.count_ones() as usize
    }

    /// Whether every slot is in flight (the next `try_alloc` will return `None`).
    pub fn is_full(&self) -> bool {
        self.in_flight() == self.capacity()
    }

    /// Whether no slots are in flight.
    pub fn is_empty(&self) -> bool {
        self.busy == 0
    }

    /// Total submissions handed out (also the value of the next ticket).
    pub fn submitted(&self) -> u64 {
        self.submitted
    }

    /// Total completions — the monotonic value a completion fence is signalled to.
    pub fn completed(&self) -> u64 {
        self.completed
    }

    /// Reserve a free slot round-robin and assign it the next ticket. Returns `None` when the
    /// ring is full (backpressure: the caller waits for a completion, then retries).
    pub fn try_alloc(&mut self) -> Option<(Slot, Ticket)> {
        if self.is_full() {
            return None;
        }
        // Round-robin scan from `next`; guaranteed to find a free slot since !is_full.
        for off in 0..self.slots {
            let idx = (self.next + off) % self.slots;
            if self.busy & (1 << idx) == 0 {
                self.busy |= 1 << idx;
                let ticket = self.submitted;
                self.ticket_of[idx as usize] = ticket;
                self.submitted += 1;
                self.next = (idx + 1) % self.slots;
                return Some((Slot(idx), Ticket(ticket)));
            }
        }
        // Unreachable given the !is_full guard, but never panic on the hot path.
        None
    }

    /// Mark `slot` complete: free it and advance the completion count. Returns the freed
    /// slot's ticket. Errors (no panic) on an out-of-range or not-in-flight slot.
    pub fn complete(&mut self, slot: Slot) -> Result<Ticket, RingError> {
        if slot.0 >= self.slots {
            return Err(RingError::InvalidSlot);
        }
        let bit = 1u32 << slot.0;
        if self.busy & bit == 0 {
            return Err(RingError::NotInFlight);
        }
        self.busy &= !bit;
        self.completed += 1;
        Ok(Ticket(self.ticket_of[slot.0 as usize]))
    }

    /// The ticket currently occupying `slot`, if it is in flight.
    pub fn ticket_of(&self, slot: Slot) -> Option<Ticket> {
        if slot.0 < self.slots && (self.busy & (1 << slot.0)) != 0 {
            Some(Ticket(self.ticket_of[slot.0 as usize]))
        } else {
            None
        }
    }

    /// Whether `slot` is currently in flight (reserved, not yet completed).
    pub fn is_in_flight(&self, slot: Slot) -> bool {
        slot.0 < self.slots && (self.busy & (1 << slot.0)) != 0
    }

    /// Abandon a single in-flight slot **without** counting it as a completion — the
    /// driver gave up on this submission (e.g. a per-slot lost-IRQ timeout). Frees the slot
    /// for reuse but leaves `completed()` unchanged, so a fence mirrored to it never claims
    /// the abandoned work finished. No-op (returns `false`) if the slot wasn't in flight.
    pub fn abandon(&mut self, slot: Slot) -> bool {
        if slot.0 >= self.slots {
            return false;
        }
        let bit = 1u32 << slot.0;
        if self.busy & bit == 0 {
            return false;
        }
        self.busy &= !bit;
        true
    }

    /// Abandon **all** in-flight slots without counting them as completions.
    ///
    /// This is the degraded-recovery escape hatch for a device that has lost a completion
    /// notification (e.g. a dropped GPU IRQ): rather than wedge forever, the driver gives up
    /// on the stuck in-flight set and resyncs. The monotonic `submitted`/`completed` counters
    /// are intentionally left unchanged (those submissions did not actually complete), so a
    /// fence mirrored to `completed()` never jumps forward over work that was abandoned.
    pub fn reset(&mut self) {
        self.busy = 0;
        self.next = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clamps_capacity() {
        assert_eq!(SubmitRing::new(0).capacity(), 1);
        assert_eq!(SubmitRing::new(4).capacity(), 4);
        assert_eq!(SubmitRing::new(999).capacity(), MAX_SLOTS);
    }

    #[test]
    fn fresh_ring_is_empty() {
        let r = SubmitRing::new(4);
        assert!(r.is_empty());
        assert!(!r.is_full());
        assert_eq!(r.in_flight(), 0);
        assert_eq!(r.submitted(), 0);
        assert_eq!(r.completed(), 0);
    }

    #[test]
    fn alloc_hands_out_monotonic_tickets_then_backpressures() {
        let mut r = SubmitRing::new(3);
        let (s0, t0) = r.try_alloc().unwrap();
        let (s1, t1) = r.try_alloc().unwrap();
        let (s2, t2) = r.try_alloc().unwrap();
        assert_eq!((t0, t1, t2), (Ticket(0), Ticket(1), Ticket(2)));
        // distinct slots
        assert_ne!(s0, s1);
        assert_ne!(s1, s2);
        assert_ne!(s0, s2);
        assert!(r.is_full());
        // Full ring → backpressure, no panic, no extra ticket consumed.
        assert_eq!(r.try_alloc(), None);
        assert_eq!(r.submitted(), 3);
    }

    #[test]
    fn complete_frees_slot_and_counts() {
        let mut r = SubmitRing::new(2);
        let (s0, t0) = r.try_alloc().unwrap();
        let (_s1, _t1) = r.try_alloc().unwrap();
        assert!(r.is_full());
        assert_eq!(r.complete(s0).unwrap(), t0);
        assert_eq!(r.completed(), 1);
        assert!(!r.is_full());
        // A freed slot can be reused; the new ticket is monotonic (not the old one).
        let (_s, t) = r.try_alloc().unwrap();
        assert_eq!(t, Ticket(2));
    }

    #[test]
    fn out_of_order_completion_is_allowed() {
        let mut r = SubmitRing::new(4);
        let (s0, _) = r.try_alloc().unwrap();
        let (_s1, _) = r.try_alloc().unwrap();
        let (s2, _) = r.try_alloc().unwrap();
        // Complete slot 2 before slot 0 — the ring tracks freedom, not order.
        r.complete(s2).unwrap();
        r.complete(s0).unwrap();
        assert_eq!(r.in_flight(), 1);
        assert_eq!(r.completed(), 2);
    }

    #[test]
    fn double_complete_and_bad_slot_error_not_panic() {
        let mut r = SubmitRing::new(2);
        let (s0, _) = r.try_alloc().unwrap();
        r.complete(s0).unwrap();
        assert_eq!(r.complete(s0), Err(RingError::NotInFlight));
        assert_eq!(r.complete(Slot(99)), Err(RingError::InvalidSlot));
    }

    #[test]
    fn ticket_of_reports_in_flight_only() {
        let mut r = SubmitRing::new(2);
        let (s0, t0) = r.try_alloc().unwrap();
        assert_eq!(r.ticket_of(s0), Some(t0));
        r.complete(s0).unwrap();
        assert_eq!(r.ticket_of(s0), None);
        assert_eq!(r.ticket_of(Slot(99)), None);
    }

    #[test]
    fn is_in_flight_tracks_reservation() {
        let mut r = SubmitRing::new(2);
        let (s0, _) = r.try_alloc().unwrap();
        assert!(r.is_in_flight(s0));
        assert!(!r.is_in_flight(Slot(99)));
        r.complete(s0).unwrap();
        assert!(!r.is_in_flight(s0));
    }

    #[test]
    fn abandon_frees_one_slot_without_counting() {
        let mut r = SubmitRing::new(4);
        let (s0, _) = r.try_alloc().unwrap();
        let (s1, _) = r.try_alloc().unwrap();
        assert!(r.abandon(s0));
        assert!(!r.is_in_flight(s0));
        assert!(r.is_in_flight(s1)); // other slots untouched
        assert_eq!(r.completed(), 0); // abandon does NOT count as a completion
                                      // Idempotent / safe on a free or bad slot.
        assert!(!r.abandon(s0));
        assert!(!r.abandon(Slot(99)));
    }

    #[test]
    fn reset_abandons_in_flight_without_counting_completions() {
        let mut r = SubmitRing::new(4);
        let (_s0, _) = r.try_alloc().unwrap();
        let (_s1, _) = r.try_alloc().unwrap();
        let submitted_before = r.submitted();
        r.reset();
        // Everything is free again, but no spurious completions were counted.
        assert!(r.is_empty());
        assert_eq!(r.in_flight(), 0);
        assert_eq!(r.completed(), 0);
        // `submitted` (the ticket counter) is monotonic — reset does not rewind it.
        assert_eq!(r.submitted(), submitted_before);
        // The ring is usable again immediately.
        assert!(r.try_alloc().is_some());
    }

    #[test]
    fn full_drain_refill_cycles_reuse_slots() {
        // Stress the round-robin reuse + bitmask over several full/drain cycles.
        let mut r = SubmitRing::new(MAX_SLOTS);
        for _cycle in 0..4 {
            let mut slots = [Slot(0); MAX_SLOTS];
            for s in slots.iter_mut() {
                *s = r.try_alloc().unwrap().0;
            }
            assert!(r.is_full());
            assert_eq!(r.try_alloc(), None);
            for s in slots {
                r.complete(s).unwrap();
            }
            assert!(r.is_empty());
        }
        assert_eq!(r.submitted(), (MAX_SLOTS * 4) as u64);
        assert_eq!(r.completed(), (MAX_SLOTS * 4) as u64);
    }
}
