// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: boot proof of the copies into and out of a memory object without the kernel lock
//! (TASK-0330 P4, `syscall::api::vmo_copy`). What the kernel lock kept apart is kept apart by
//! the object's own count now, and each half is shown here: a copy in flight (the count
//! `vmo_write`/`vmo_read` take for their length) refuses a seal, an unlock for purging and a
//! destroy, each of which goes through once the copy is over — and the syscalls themselves are
//! refused by a sealed object (a write; a read goes through) and by an unlocked one.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: vmo copy ok (reject=seal,unlock,destroy,sealed,unlocked)` on
//!   every QEMU lane and on the board
//! ADR: docs/adr/0103-a-fault-holds-its-own-address-space-and-nothing-else-a-shootdown-reaches-the-harts-that-hold-the-space.md

use super::Context;
use crate::cap::CapError;
use crate::mm::vmo::{self, mapping, purge, SealError, VmoError, VmoKind};
use crate::mm::PAGE_SIZE;
use crate::syscall::{
    api, Args, Error as SysError, SyscallTable, SYSCALL_VMO_READ, SYSCALL_VMO_WRITE,
};
use crate::task::Pid;
use crate::vmo_lifetime::PurgeRefusal;

/// Bootstrap cap slots above the pressure selftests' (56, 57).
const EAGER_SLOT: usize = 58;
const LAZY_SLOT: usize = 59;

/// A copy of no bytes: the body runs whole — the capability, the count, the span — and touches
/// no memory of the caller's (a kernel-mode selftest task has no user buffer).
fn copy(table: &SyscallTable, sys: &mut api::Context<'_>, nr: usize, slot: usize) -> Verdict {
    match table.dispatch(nr, sys, &Args::new([slot, 0, 0, 0, 0, 0])) {
        Ok(0) => Verdict::Done,
        Err(SysError::Capability(CapError::PermissionDenied)) => Verdict::Denied,
        _ => Verdict::Other,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Done,
    Denied,
    Other,
}

pub(super) fn run_vmo_copy_selftest(ctx: &mut Context<'_>) {
    ctx.tasks.set_current(Pid::KERNEL);
    let caller = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let reach = crate::dma_reach::DmaReach::ALL;
    let (Ok((eager, f0)), Ok((lazy, f1)), Ok((gone, f2))) = (
        vmo::create(PAGE_SIZE, VmoKind::Anon, &reach),
        vmo::create(PAGE_SIZE, VmoKind::Lazy, &reach),
        vmo::create(PAGE_SIZE, VmoKind::Anon, &reach),
    ) else {
        log_error!(target: "selftest", "KSELFTEST: vmo copy FAIL (create)");
        return;
    };
    // The caller holds the two it copies through, as a service holds its objects.
    let held = ctx.tasks.caps_of_mut(caller).is_some_and(|mut caps| {
        caps.set(EAGER_SLOT, f0).is_ok() && caps.set(LAZY_SLOT, f1).is_ok()
    });

    // A write in flight is a writer: no seal until it is over.
    let writing = mapping::begin(eager, true);
    let seal_refused = writing.is_ok() && vmo::seal(eager) == Err(SealError::MappedWritable);
    drop(writing);
    // A read in flight reaches the pages: no unlock for purging until it is over.
    let reading = mapping::begin(lazy, false);
    let unlock_refused = reading.is_ok()
        && purge::unlock(lazy) == Err(purge::PurgeError::Refused(PurgeRefusal::Mapped));
    drop(reading);
    // Nobody holds the third object any more — the copy still reaches it: no destroy.
    let reaching = mapping::begin(gone, false);
    drop(f2);
    let destroy_refused = reaching.is_ok() && vmo::destroy(gone) == Err(VmoError::Mapped);
    drop(reaching);
    let destroyed = vmo::destroy(gone).is_ok();

    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys = api::Context::new(
        ctx.scheduler,
        ctx.tasks,
        ctx.router,
        ctx.address_spaces,
        timer,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    );
    sys.tasks.set_current(caller);
    // The syscalls, end to end: an ordinary object takes both; sealed, it has no writer and is
    // still read; unlocked for purging, it is nobody's to touch until its lock.
    let plain = (
        copy(&table, &mut sys, SYSCALL_VMO_WRITE, EAGER_SLOT),
        copy(&table, &mut sys, SYSCALL_VMO_READ, LAZY_SLOT),
    );
    let sealed = vmo::seal(eager);
    let sealed_write = copy(&table, &mut sys, SYSCALL_VMO_WRITE, EAGER_SLOT);
    let sealed_read = copy(&table, &mut sys, SYSCALL_VMO_READ, EAGER_SLOT);
    let unlocked = purge::unlock(lazy);
    let unlocked_read = copy(&table, &mut sys, SYSCALL_VMO_READ, LAZY_SLOT);
    let unlocked_write = copy(&table, &mut sys, SYSCALL_VMO_WRITE, LAZY_SLOT);
    let relocked = purge::lock(lazy);
    // Each call counted itself off: nothing reaches the objects once the calls are over.
    let settled = mapping::spaces(eager) == 0 && mapping::spaces(lazy) == 0;

    sys.tasks.set_current(Pid::KERNEL);
    if let Some(mut caps) = sys.tasks.caps_of_mut(caller) {
        let _ = caps.take(EAGER_SLOT);
        let _ = caps.take(LAZY_SLOT);
    }
    for id in [eager, lazy] {
        let _ = vmo::destroy(id);
    }

    if held
        && seal_refused
        && unlock_refused
        && destroy_refused
        && destroyed
        && plain == (Verdict::Done, Verdict::Done)
        && sealed == Ok(())
        && sealed_write == Verdict::Denied
        && sealed_read == Verdict::Done
        && unlocked == Ok(())
        && unlocked_read == Verdict::Denied
        && unlocked_write == Verdict::Denied
        && relocked.is_ok()
        && settled
    {
        log_info!(
            target: "selftest",
            "KSELFTEST: vmo copy ok (reject=seal,unlock,destroy,sealed,unlocked)"
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: vmo copy FAIL (held={} seal={} unlock={} destroy={} destroyed={} plain={:?} sealed={:?} sealed_write={:?} sealed_read={:?} unlocked={:?} unlocked_read={:?} unlocked_write={:?} relocked={:?} settled={})",
            held,
            seal_refused,
            unlock_refused,
            destroy_refused,
            destroyed,
            plain,
            sealed,
            sealed_write,
            sealed_read,
            unlocked,
            unlocked_read,
            unlocked_write,
            relocked,
            settled
        );
    }
}
