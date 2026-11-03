// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ADR-0091's negative proofs at boot, through the syscall table every process uses —
//! `KSELFTEST: ambient authority reject ok (`. The retired `as_create`/`as_map` numbers answer
//! nothing; `spawn` refuses another process's space and a supervisor task; a lazy object larger
//! than the memory that could back it is `EINVAL`; sixty-four pool-sized lazy objects cost the
//! kernel heap their chunk tables only and give them back (ADR-0093: reserving costs nothing per
//! page); a size no arena of the heap could hold is refused without asking the pool — never a
//! kernel panic.
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
/// Pool-sized lazy objects reserved at once: more than any heap could hold the words of (64 MiB
/// of them for a 1 GiB pool), a quarter MiB of chunk tables since ADR-0093.
const MAX_HEAP_PROBES: usize = 64;
/// The kernel heap the probes may cost together (TASK-0352 P6): each object's chunk table — a
/// fat pointer per 1024 pages of the pool — and 4 KiB of slack, on a pool of `pool_bytes` (the
/// 1 GiB lane's objects cost 4.3 KiB each; a fixed 4 KiB refused them).
const fn sparse_budget(pool_bytes: usize) -> usize {
    MAX_HEAP_PROBES * (pool_bytes / 4096 / 1024 * 16 + 4096)
}

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

    // ADR-0093 (TASK-0352 P6): pool-sized lazy objects reserved cost the heap their chunk tables
    // only, and give them back; the reserve that used to refuse their words is not reached by any
    // size the pool admits. A size no arena of the heap could hold is refused without asking the
    // pool (the pool's frames stay where they are).
    let used_before = crate::kheap::used_bytes();
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
    let objects = held.len();
    let heap_delta = crate::kheap::used_bytes().saturating_sub(used_before);
    let sparse = refusal.is_none() && heap_delta <= sparse_budget(pool_bytes);
    for slot in held {
        let _ =
            table.dispatch(SYSCALL_VMO_DESTROY, &mut sys_ctx, &Args::new([slot, 0, 0, 0, 0, 0]));
    }
    let refused = !crate::kheap::admits(usize::MAX / 2);

    if retired == RETIRED.len() && spawn_refused == 2 && bound && sparse && refused {
        log_info!(
            target: "selftest",
            "KSELFTEST: ambient authority reject ok (retired={} spawn={} bound=1 sparse=1 refused=1 objects={} heap_delta={})",
            retired,
            spawn_refused,
            objects,
            heap_delta
        );
    } else {
        log_info!(
            target: "selftest",
            "KSELFTEST: ambient authority reject FAIL (retired={} spawn={} bound={} sparse={} refused={} objects={} heap_delta={} refusal={:?})",
            retired,
            spawn_refused,
            bound,
            sparse,
            refused,
            objects,
            heap_delta,
            refusal
        );
    }
}
