// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic timeline-fence selftests (KSELFTEST markers): the
//! RFC-0033 create/signal/wait/timeout proof and the ADR-0062 rights proof
//! (a WAIT-only fence copy may wait but never signal). Split out of
//! `selftest/mod.rs` (structure-gate); runs from `selftest::entry`.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker contract (`KSELFTEST: fence wait ok`,
//!   `KSELFTEST: fence timeout ok`, `KSELFTEST: fence transfer ok`)
//! ADR: docs/adr/0062-boot-stage-fence-and-ready-messages.md

use super::Context;
use crate::hal::Timer;
use crate::syscall::{
    api, Args, Error as SysError, SyscallTable, SYSCALL_CAP_CLOSE, SYSCALL_CAP_TRANSFER,
    SYSCALL_FENCE_CREATE, SYSCALL_FENCE_SIGNAL, SYSCALL_FENCE_WAIT,
};
use crate::task::Pid;
use crate::{log_error, log_info};

/// RFC-0033 runtime proof: exercises the fence syscalls end-to-end in QEMU. Proves create,
/// monotonic signal (a lower signal does not lower the value), the immediately-satisfied
/// wait path, and the deadline/timeout path — all against the real table + block/wake
/// machinery (`fence_signal` reuses the shared `tasks.wake` path).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(super) fn run_fence_selftest(ctx: &mut Context<'_>) {
    use crate::ipc::IpcError;

    ctx.tasks.set_current(Pid::KERNEL);

    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx = api::Context::new(
        ctx.scheduler,
        ctx.tasks,
        ctx.router,
        ctx.address_spaces,
        timer,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    );

    let fence_slot = match table.dispatch(SYSCALL_FENCE_CREATE, &mut sys_ctx, &Args::new([0; 6])) {
        Ok(slot) => slot,
        Err(e) => {
            log_error!(target: "selftest", "KSELFTEST: fence FAIL create={:?}", e);
            return;
        }
    };

    // Signal to 10, then a wait for target 5 is satisfied immediately (no block) → Ok.
    if let Err(e) =
        table.dispatch(SYSCALL_FENCE_SIGNAL, &mut sys_ctx, &Args::new([fence_slot, 10, 0, 0, 0, 0]))
    {
        log_error!(target: "selftest", "KSELFTEST: fence FAIL signal={:?}", e);
        return;
    }
    let wait_ok = table
        .dispatch(SYSCALL_FENCE_WAIT, &mut sys_ctx, &Args::new([fence_slot, 5, 0, 0, 0, 0]))
        .is_ok();

    // Monotonic: a lower signal (3) must NOT lower the value, so a wait for 10 still passes.
    let _ =
        table.dispatch(SYSCALL_FENCE_SIGNAL, &mut sys_ctx, &Args::new([fence_slot, 3, 0, 0, 0, 0]));
    let mono_ok = table
        .dispatch(SYSCALL_FENCE_WAIT, &mut sys_ctx, &Args::new([fence_slot, 10, 0, 0, 0, 0]))
        .is_ok();

    if wait_ok && mono_ok {
        log_info!(target: "selftest", "KSELFTEST: fence wait ok");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: fence wait FAIL: wait_ok={} mono_ok={}",
            wait_ok,
            mono_ok
        );
    }

    // Timeout: an unreachable target (999) with an already-elapsed deadline → TimedOut
    // (checked before any block, so no hang).
    let past_deadline = timer.now() as usize;
    match table.dispatch(
        SYSCALL_FENCE_WAIT,
        &mut sys_ctx,
        &Args::new([fence_slot, 999, past_deadline, 0, 0, 0]),
    ) {
        Err(SysError::Ipc(IpcError::TimedOut)) => {
            log_info!(target: "selftest", "KSELFTEST: fence timeout ok")
        }
        other => {
            log_error!(target: "selftest", "KSELFTEST: fence timeout FAIL: {:?}", other)
        }
    }

    let _ =
        table.dispatch(SYSCALL_CAP_CLOSE, &mut sys_ctx, &Args::new([fence_slot, 0, 0, 0, 0, 0]));
}

/// ADR-0062 (TASK-0324 P5-a): the boot-stage fence a child receives may WAIT but never SIGNAL.
/// Proves the rights mask against the real syscall table: `fence_create` mints `MANAGE | WAIT`,
/// a WAIT-only copy is derivable (and thus transferable to a child), that copy waits
/// successfully, and its signal is refused — a child that could release a stage for the whole
/// fleet is the security defect this mask closes.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(super) fn run_fence_rights_selftest(ctx: &mut Context<'_>) {
    use crate::cap::Rights;

    ctx.tasks.set_current(Pid::KERNEL);

    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx = api::Context::new(
        ctx.scheduler,
        ctx.tasks,
        ctx.router,
        ctx.address_spaces,
        timer,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    );

    let owner = match table.dispatch(SYSCALL_FENCE_CREATE, &mut sys_ctx, &Args::new([0; 6])) {
        Ok(slot) => slot,
        Err(e) => {
            log_error!(target: "selftest", "KSELFTEST: fence transfer FAIL create={:?}", e);
            return;
        }
    };
    // What init hands a child: WAIT only — a subset of the creator's MANAGE | WAIT.
    let child_view =
        Args::new([Pid::KERNEL.as_raw() as usize, owner, Rights::WAIT.bits() as usize, 0, 0, 0]);
    let waiter = match table.dispatch(SYSCALL_CAP_TRANSFER, &mut sys_ctx, &child_view) {
        Ok(slot) => slot,
        Err(e) => {
            log_error!(target: "selftest", "KSELFTEST: fence transfer FAIL derive={:?}", e);
            let _ =
                table.dispatch(SYSCALL_CAP_CLOSE, &mut sys_ctx, &Args::new([owner, 0, 0, 0, 0, 0]));
            return;
        }
    };

    let signalled = table
        .dispatch(SYSCALL_FENCE_SIGNAL, &mut sys_ctx, &Args::new([owner, 4, 0, 0, 0, 0]))
        .is_ok();
    let waited = table
        .dispatch(SYSCALL_FENCE_WAIT, &mut sys_ctx, &Args::new([waiter, 4, 0, 0, 0, 0]))
        .is_ok();
    let refused = matches!(
        table.dispatch(SYSCALL_FENCE_SIGNAL, &mut sys_ctx, &Args::new([waiter, 9, 0, 0, 0, 0])),
        Err(SysError::Capability(_))
    );

    if signalled && waited && refused {
        log_info!(target: "selftest", "KSELFTEST: fence transfer ok");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: fence transfer FAIL signal={} wait={} refused={}",
            signalled,
            waited,
            refused
        );
    }

    for slot in [owner, waiter] {
        let _ = table.dispatch(SYSCALL_CAP_CLOSE, &mut sys_ctx, &Args::new([slot, 0, 0, 0, 0, 0]));
    }
}
