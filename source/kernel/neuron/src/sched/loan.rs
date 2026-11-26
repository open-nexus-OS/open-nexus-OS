// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a time right on loan (TASK-0330 P2, ADR-0102; RFC-0120). A caller that runs with a
//! reservation lends it to the thread its call wakes: the server is placed on the reservation's
//! hart, arrives there with its deadline and what is left of its budget, and what it runs is
//! charged to it — nothing is admitted, nothing is added. One reservation, one thread running
//! with it: the loan ends when the lender is woken or its call returns ([`take_back`]), and when
//! the borrower waits for anything but the answer to a call of its own, or exits
//! ([`give_back`]). A borrower that calls on lends what it runs with; taking back follows the
//! chain.
//!
//! The words live in the two tasks' scheduling records (`sync::loan_word::Loan`): no lock, a
//! caller and a waker on different harts settle them with one compare-and-swap. A loan that
//! loses a race is not made — the server then runs as it did before there were loans.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the words' rule in `sync::loan_word` (host); `KSELFTEST: time right loan ok`
//!   and `test_reject_time_right_loan` (`selftest/smp_sched/loan.rs`) through these functions,
//!   the wake and the block's first step; `lent=` on the `KINIT: time rights` line (the call path)
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use core::sync::atomic::AtomicUsize;

use super::*;

/// How far taking back follows a chain of loans (a server that called on, and so on): past
/// it a borrower keeps what it has until it waits.
const CHAIN: usize = 8;

/// Loans made since boot.
static LENT: AtomicUsize = AtomicUsize::new(0);

/// Loans made since boot (the `KINIT: time rights` line).
pub fn count() -> usize {
    LENT.load(Ordering::Relaxed)
}

/// Whether task `pid` runs with a time right on loan.
pub fn borrows(pid: TaskId) -> bool {
    record::get(pid).is_some_and(|rec| rec.loan().borrowed() != 0)
}

/// `caller`'s call is about to wake `server`: the server runs with the caller's time right
/// until the caller runs again. Before the wake, so the wake places the server by it. Nothing
/// is lent by a caller that runs with none, to a server that holds one of its own or one on
/// loan, or to one that may not run on the reservation's hart.
pub fn lend(caller: TaskId, server: TaskId) {
    let Some(from) = record::get(caller) else { return };
    let raw = reserve::runs_with(from);
    if raw == 0 || caller == server {
        return;
    }
    let Some(to) = record::get(server) else { return };
    let may_run_there = reserve::hart_of_raw(raw).is_some_and(|h| to.affinity().contains(h));
    if to.reservation() != 0 || !may_run_there {
        return;
    }
    // A loan this caller left open (a call that ended while its server ran on) ends first.
    take_back(caller);
    if to.loan().take(caller.as_raw(), raw) {
        from.loan().lent(server.as_raw());
        LENT.fetch_add(1, Ordering::Relaxed);
    }
}

/// `lender` runs again — it was woken, its call returned, its reservation changes or it exits:
/// what it lent is its own again, and what its borrower lent on with it.
pub fn take_back(lender: TaskId) {
    let mut lender = lender;
    for _ in 0..CHAIN {
        let Some(borrower) = take_back_one(lender) else { return };
        lender = borrower;
    }
}

/// One link of [`take_back`]: the task `lender` lent to, if it did.
fn take_back_one(lender: TaskId) -> Option<TaskId> {
    let to = record::get(lender)?.loan().take_lent();
    if to == 0 {
        return None;
    }
    let (borrower, from) = (TaskId::from_raw(to), lender.as_raw());
    let rec = record::get(borrower)?;
    if borrower == runs_here() {
        // The reply: the borrower's own send wakes its lender. It runs here and registers
        // nowhere meanwhile, so nobody lends to it between the look and the end.
        if rec.loan().lender() == from {
            settle_here(borrower, rec);
            let _ = rec.loan().end(from);
        }
    } else if rec.loan().end(from) {
        // Wherever it is: its record names the reservation no more, so no arrival takes it up
        // again; the time since its last charge on another hart stays its own.
        for cpu in 0..RUN_QUEUES.len() {
            let _ = meter::metered(cpu, |q| drop_copy(q, borrower, rec));
        }
    }
    Some(borrower)
}

/// `me` — this hart's task, or one that runs nowhere (an exit) — runs with a time right on loan
/// no more: it waits for something that is not the answer to a call of its own, or it is gone.
pub fn give_back(me: TaskId) {
    let Some(rec) = record::get(me) else { return };
    let lender = rec.loan().lender();
    if lender != 0 {
        settle_here(me, rec);
        let _ = rec.loan().end(lender);
    }
}

/// This hart's task `id` is charged to now with the loan still in its record — the charge is
/// the reservation's — and its queue entry loses the copy.
fn settle_here(id: TaskId, rec: &record::Record) {
    let cpu = crate::smp::cpu_current_id().as_index();
    let _ = meter::metered(cpu, |q| drop_copy(q, id, rec));
}

/// Task `id`'s entry in `q` runs with no reservation — unless it holds one of its own by now (a
/// bind since the loan): then the entry's copy is that one's.
fn drop_copy(q: &mut HartQueues, id: TaskId, rec: &record::Record) {
    if rec.reservation() == 0 {
        let _ = q.set_res(id.as_raw(), None);
    }
}
