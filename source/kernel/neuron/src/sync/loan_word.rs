// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the words of a time right on loan (TASK-0330 P2, ADR-0102) — a task's part in a
//! loan as the borrower (the reservation it runs with, and whose it is) and as the lender (whom
//! it lent to). They live in the task's scheduling record and are settled without a lock: a
//! caller lending, a waker taking back and the borrower giving back may meet on three harts, and
//! one compare-and-swap decides who ended a loan. What the words mean to the scheduler is
//! `sched::loan`.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file (host): a loan read whole or not at all, one loan at a time, ended
//!   by its lender's name only, the lender's word taken once
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// The lender word while a loan's reservation is being written (no task has this id).
const CLAIMED: u32 = u32::MAX;

/// A task's part in a loan.
pub struct Loan {
    borrowed: AtomicU64,
    lender: AtomicU32,
    lent_to: AtomicU32,
}

impl Loan {
    pub const fn new() -> Self {
        Self { borrowed: AtomicU64::new(0), lender: AtomicU32::new(0), lent_to: AtomicU32::new(0) }
    }

    /// The borrower takes reservation `raw` on loan from task `lender`. `false`: it runs with
    /// one on loan already.
    pub fn take(&self, lender: u32, raw: u64) -> bool {
        if self.lender.compare_exchange(0, CLAIMED, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return false;
        }
        self.borrowed.store(raw, Ordering::Release);
        self.lender.store(lender, Ordering::Release);
        true
    }

    /// Whose reservation the borrower runs with (0: nobody's).
    pub fn lender(&self) -> u32 {
        match self.lender.load(Ordering::Acquire) {
            CLAIMED => 0,
            lender => lender,
        }
    }

    /// The reservation the borrower runs with on loan (raw; 0: none).
    pub fn borrowed(&self) -> u64 {
        if self.lender() == 0 {
            0
        } else {
            self.borrowed.load(Ordering::Acquire)
        }
    }

    /// The borrower's loan from task `lender` ends. `false`: it had none from that task (another
    /// hart ended it first).
    pub fn end(&self, lender: u32) -> bool {
        lender != 0
            && lender != CLAIMED
            && self.lender.compare_exchange(lender, 0, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }

    /// The borrower's loan ends, whoever lent it (its reservation names nothing any more).
    pub fn end_any(&self) -> bool {
        self.end(self.lender())
    }

    /// The lender lent to task `to`.
    pub fn lent(&self, to: u32) {
        self.lent_to.store(to, Ordering::Release);
    }

    /// Whom the lender lent to (0: nobody), once: the hart that reads it takes the loan back.
    pub fn take_lent(&self) -> u32 {
        if self.lent_to.load(Ordering::Acquire) == 0 {
            0
        } else {
            self.lent_to.swap(0, Ordering::AcqRel)
        }
    }
}

impl Default for Loan {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loan_is_read_whole_or_not_at_all() {
        let loan = Loan::new();
        assert_eq!(loan.borrowed(), 0);
        assert!(loan.take(7, 0x1_0003));
        assert_eq!((loan.lender(), loan.borrowed()), (7, 0x1_0003));
        // A claim in progress reads as no loan: the reservation word is not written yet.
        loan.lender.store(CLAIMED, Ordering::Release);
        assert_eq!((loan.lender(), loan.borrowed()), (0, 0));
    }

    #[test]
    fn test_reject_a_second_loan_and_an_end_by_a_stranger() {
        let loan = Loan::new();
        assert!(loan.take(7, 11));
        assert!(!loan.take(8, 12), "one loan at a time");
        assert_eq!(loan.borrowed(), 11);
        assert!(!loan.end(8), "only its lender's end ends it");
        assert!(!loan.end(0) && !loan.end(CLAIMED));
        assert!(loan.end(7));
        assert!(!loan.end(7), "ended once");
        assert_eq!(loan.borrowed(), 0);
        assert!(loan.take(8, 12));
        assert!(loan.end_any() && !loan.end_any());
    }

    #[test]
    fn the_lender_s_word_is_taken_once() {
        let loan = Loan::new();
        assert_eq!(loan.take_lent(), 0);
        loan.lent(9);
        assert_eq!((loan.take_lent(), loan.take_lent()), (9, 0));
    }
}
