// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a time right on loan (TASK-0330 P2, ADR-0102) through the kernel's own paths — the
//! lend a call's send makes, the wake and the block's first step that end it, the scheduler's
//! arrival and meter that run a borrower by it. Two proofs. The loan runs: a server lent a
//! fifth of the boot hart, against a hog of eight times its weight, runs twenty budgets in full
//! — counted where a holder's are — and none once its lender took the right back. And the rule:
//! who is lent nothing; the loan placed on the reservation's hart; ended by the lender's wake,
//! by the borrower's wait for anything but its own call's answer, down a chain of calls, by the
//! lender's unbind and its exit; the books balanced after.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `KSELFTEST: time right loan ok`, `KSELFTEST: test_reject_time_right_loan ok`
//!   on the multi-hart lanes and the board
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use super::super::Context;
use super::time_right::{drive, queued, verdict, PERIOD_NS, SHARE_PPM};
use crate::hal::platform::ns_to_ticks;
use crate::sched::{loan, meter, reserve, QosClass};
use crate::task::wake::{abort_block, prepare_block};
use crate::task::{BlockReason, Pid};
use crate::types::CpuId;
use crate::{log_error, log_info};
use nexus_sched::CpuSet;

/// The budgets the borrower is driven through.
const BUDGETS: usize = 20;

/// The wait of a caller for its call's answer, and of a server for its next request.
const CALL: BlockReason = BlockReason::IpcCall { reply_ep: 0 };
const RECV: BlockReason = BlockReason::IpcRecv { endpoint: 0, deadline_ns: 0 };

pub(super) fn run(ctx: &mut Context<'_>, boot: CpuId, other: CpuId) {
    let [caller, server, next, holder, crowd] =
        [(); 5].map(|()| ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler));
    let root = reserve::selftest_root();
    let before = reserve::held(root);
    let Ok(right) = reserve::derive(root, 2 * SHARE_PPM, 0) else {
        log_error!(target: "selftest", "KSELFTEST: time right loan FAIL (derive)");
        return;
    };
    let on_boot = || core::iter::once(boot.as_index());
    let bound = reserve::bind(caller, right, SHARE_PPM, PERIOD_NS, on_boot()).is_ok()
        && reserve::bind(holder, right, SHARE_PPM, PERIOD_NS, on_boot()).is_ok();
    let lent0 = loan::count();

    // Who is lent nothing: by a caller that runs with no reservation; a server that holds its
    // own; one that may not run where the reservation is admitted.
    loan::lend(server, next);
    let none_to_lend = !loan::borrows(next);
    loan::lend(caller, holder);
    let holds_its_own = !loan::borrows(holder);
    let (rec, may) = match crate::sched::record::get(next) {
        Some(rec) => (rec, rec.affinity()),
        None => return,
    };
    rec.set_affinity(CpuSet::single(other.as_index()).unwrap_or(may));
    loan::lend(caller, next);
    let off_its_hart = other == boot || !loan::borrows(next);
    rec.set_affinity(may);
    verdict("loan", bound && none_to_lend && holds_its_own && off_its_hart);

    // The loan runs: the server arrives with the caller's deadline and budget, its budgets run
    // out as a holder's do — and no more once the caller took its right back.
    ctx.scheduler.selftest_reset_cpu(boot);
    loan::lend(caller, server);
    let placed = reserve::hart_of(server) == Some(boot.as_index());
    let (overruns0, ran0) = (reserve::counts().0, meter::runtime_ns(server));
    let ready = queued(ctx, boot, server, QosClass::Normal)
        && queued(ctx, boot, crowd, QosClass::PerfBurst);
    let limit = 4 * BUDGETS as u64 * ns_to_ticks(PERIOD_NS);
    let _ = drive(ctx, boot, limit, || reserve::counts().0 - overruns0 >= BUDGETS);
    let budgets = reserve::counts().0 - overruns0;
    let ran_us = (meter::runtime_ns(server) - ran0) / 1_000;
    let budget_us = PERIOD_NS / 5 / 1_000;
    loan::take_back(caller);
    let taken = reserve::counts().0;
    let _ = drive(ctx, boot, BUDGETS as u64 * ns_to_ticks(PERIOD_NS) / 2, || false);
    let after = reserve::counts().0 - taken;
    ctx.scheduler.selftest_reset_cpu(boot);
    ctx.scheduler.purge(server);
    ctx.scheduler.purge(crowd);
    let ran = placed && ready && budgets >= BUDGETS && ran_us + 1 >= budgets as u64 * budget_us;

    // What ends it. The reply: the lender is woken while it waits for its answer.
    loan::lend(caller, server);
    prepare_block(caller, CALL);
    let lent = loan::borrows(server);
    let _ = ctx.tasks.wake(caller, ctx.scheduler);
    abort_block(caller);
    let by_wake = lent && !loan::borrows(server);
    // The server waits for its next request without having answered.
    loan::lend(caller, server);
    prepare_block(server, RECV);
    abort_block(server);
    let by_wait = !loan::borrows(server);
    // A server that calls on keeps it and lends it on; the first lender's wake ends both.
    loan::lend(caller, server);
    prepare_block(server, CALL);
    loan::lend(server, next);
    let chained = loan::borrows(server)
        && loan::borrows(next)
        && reserve::hart_of(next) == Some(boot.as_index());
    loan::take_back(caller);
    abort_block(server);
    let by_chain = chained && !loan::borrows(server) && !loan::borrows(next);
    // The lender's reservation ends, and the lender does.
    loan::lend(caller, server);
    let by_unbind = reserve::unbind(caller, right).is_ok() && !loan::borrows(server);
    loan::lend(holder, server);
    let lent_again = loan::borrows(server);
    reserve::release(holder);
    let by_exit = lent_again && !loan::borrows(server) && !reserve::holds(holder);
    let lent = loan::count() - lent0;
    let balanced =
        reserve::revoke(right).is_ok() && reserve::audit() && reserve::held(root) == before;

    if ran && after == 0 && by_wake && by_wait && by_chain && by_unbind && by_exit && balanced {
        log_info!(
            target: "selftest",
            "KSELFTEST: time right loan ok (budgets={} of_us={} ran_us={} after_return={} lent={})",
            budgets,
            budget_us,
            ran_us,
            after,
            lent
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: time right loan FAIL (placed={} ready={} budgets={} ran_us={} after_return={} wake={} wait={} chain={} unbind={} exit={} balanced={})",
            placed,
            ready,
            budgets,
            ran_us,
            after,
            by_wake,
            by_wait,
            by_chain,
            by_unbind,
            by_exit,
            balanced
        );
    }
}
