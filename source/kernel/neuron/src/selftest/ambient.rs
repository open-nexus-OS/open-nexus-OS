// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ADR-0091's negative proofs at boot, through the syscall table every process uses —
//! `KSELFTEST: ambient authority reject ok (`. The retired `as_create`/`as_map` numbers answer
//! nothing; `spawn` refuses another process's space and a supervisor task; a lazy object larger
//! than the memory that could back it is `EINVAL`; lazy objects asked until the kernel heap
//! reaches its reserve end in `ENOMEM` — never a kernel panic — and give their heap back.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! TEST_COVERAGE: QEMU marker ladder (headless + full profiles gate it), host `kheap` tests
//! ADR: docs/adr/0091-no-ambient-authority-over-address-spaces-a-task-maps-and-spawns-only-into-its-own-the-kernel-bounds-what-a-caller-sizes.md

use super::*;
use crate::cap::CapError;
use crate::mm::AsHandle;
use crate::syscall::{SYSCALL_SPAWN, SYSCALL_VMO_DESTROY};

/// The numbers ADR-0091 retired: a fresh address space, a fixed-VA map into any space.
const RETIRED: [usize; 2] = [9, 10];
/// More pool-sized lazy objects than any heap can hold the words of: the reserve stops them.
const MAX_HEAP_PROBES: usize = 64;

/// Runs every rejection against `foreign` — an address space the bootstrap task does not run in.
pub(super) fn run_ambient_authority_selftest(ctx: &mut Context<'_>, foreign: AsHandle) {
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
    let none = Args::new([0; 6]);
    let retired = RETIRED
        .iter()
        .filter(|&&n| {
            matches!(table.dispatch(n, &mut sys_ctx, &none), Err(SysError::InvalidSyscall))
        })
        .count();

    let denied = |r: Result<usize, SysError>| {
        matches!(r, Err(SysError::Capability(CapError::PermissionDenied)))
    };
    let into_foreign = Args::new([0x1000, 0x4000_0000, foreign.to_raw() as usize, 0, 0, 0]);
    let supervisor = Args::new([0x1000, 0, 0, 0, 0, 0]);
    let spawn_refused =
        usize::from(denied(table.dispatch(SYSCALL_SPAWN, &mut sys_ctx, &into_foreign)))
            + usize::from(denied(table.dispatch(SYSCALL_SPAWN, &mut sys_ctx, &supervisor)));

    let pool_bytes =
        crate::mm::frame_pool::stats().map_or(0, |stats| stats.total.saturating_mul(PAGE_SIZE));
    let lazy = |len: usize| {
        Args::new([usize::MAX, len, api::VMO_CREATE_LAZY, api::VMO_CREATE_NO_DEVICE, 0, 0])
    };
    let over = table.dispatch(SYSCALL_VMO_CREATE, &mut sys_ctx, &lazy(pool_bytes + PAGE_SIZE));
    let bound = matches!(over, Err(SysError::AddressSpace(AddressSpaceError::InvalidArgs)));

    // Pool-sized lazy objects until the heap's reserve refuses one: the refusal is ENOMEM.
    let mut held = alloc::vec::Vec::new();
    let mut refusal = None;
    for _ in 0..MAX_HEAP_PROBES {
        match table.dispatch(SYSCALL_VMO_CREATE, &mut sys_ctx, &lazy(pool_bytes)) {
            Ok(slot) => held.push(slot),
            Err(err) => {
                refusal = Some(err);
                break;
            }
        }
    }
    let reserve = matches!(refusal, Some(SysError::AddressSpace(AddressSpaceError::OutOfFrames)));
    let objects = held.len();
    for slot in held {
        let _ =
            table.dispatch(SYSCALL_VMO_DESTROY, &mut sys_ctx, &Args::new([slot, 0, 0, 0, 0, 0]));
    }

    if retired == RETIRED.len() && spawn_refused == 2 && bound && reserve {
        log_info!(
            target: "selftest",
            "KSELFTEST: ambient authority reject ok (retired={} spawn={} bound=1 reserve=1 objects={})",
            retired,
            spawn_refused,
            objects
        );
    } else {
        log_info!(
            target: "selftest",
            "KSELFTEST: ambient authority reject FAIL (retired={} spawn={} bound={} reserve={} objects={} refusal={:?})",
            retired,
            spawn_refused,
            bound,
            reserve,
            objects,
            refusal
        );
    }
}
