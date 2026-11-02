// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The kernel selftests' set-up through the kernel's OWN API (ADR-0091): a fresh
//! address space, a VMO mapped at a fixed VA in it, a thread started in it, a supervisor task.
//! Userspace has none of these — `as_create`/`as_map` (9/10) are retired, and `spawn` (7)
//! starts a thread in the caller's own space only — so the selftests no longer reach them
//! through the syscall table.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! TEST_COVERAGE: QEMU marker ladder (`KSELFTEST: as create ok`, `as map ok`, `spawn newas ok`,
//! `exit ok`, `wait ok`)
//! ADR: docs/adr/0091-no-ambient-authority-over-address-spaces-a-task-maps-and-spawns-only-into-its-own-the-kernel-bounds-what-a-caller-sizes.md

use crate::cap::CapabilityKind;
use crate::mm::{AsHandle, PageFlags, PAGE_SIZE};
use crate::syscall::api;
use crate::types::{SlotIndex, VirtAddr};

/// A user-readable page: the selftest's data mapping.
pub(super) const USER_RO: PageFlags =
    PageFlags::VALID.union(PageFlags::USER).union(PageFlags::READ);
/// A user-writable page: the selftest child's stack.
pub(super) const USER_RW: PageFlags = USER_RO.union(PageFlags::WRITE);

/// A fresh address space (the kernel's own `AddressSpaceManager::create`).
pub(super) fn create_space(ctx: &mut api::Context<'_>) -> AsHandle {
    ctx.address_spaces.create().expect("selftest address space")
}

/// Maps the first `len` bytes of the VMO the bootstrap task holds in `slot` at `va` of `space`,
/// page by page with `flags` — what the retired `as_map` did, now the kernel's business alone.
pub(super) fn map_slot_at(
    ctx: &mut api::Context<'_>,
    space: AsHandle,
    slot: usize,
    va: usize,
    len: usize,
    flags: PageFlags,
) {
    let cap = ctx.tasks.bootstrap_mut().caps_mut().get(slot).expect("selftest vmo cap");
    let CapabilityKind::Vmo { id, .. } = cap.kind else {
        panic!("selftest slot {} holds no VMO", slot);
    };
    for page in 0..len / PAGE_SIZE {
        let pa = crate::mm::vmo::translate(id, page * PAGE_SIZE).expect("selftest vmo page");
        ctx.address_spaces
            .map_page_tracked(space, va + page * PAGE_SIZE, pa, flags)
            .expect("selftest fixed map");
    }
}

/// A thread of the bootstrap task started in `space` at `entry` on `sp` with `gp`, left
/// suspended (the selftest runs its entry directly).
pub(super) fn spawn_in(
    ctx: &mut api::Context<'_>,
    space: AsHandle,
    entry: usize,
    sp: usize,
    gp: usize,
) -> usize {
    let parent = ctx.tasks.current_pid();
    let entry = VirtAddr::instr_aligned(entry).expect("selftest entry");
    let sp = VirtAddr::new(sp).expect("selftest stack");
    ctx.tasks
        .spawn(
            parent,
            entry,
            Some(sp),
            Some(space),
            gp,
            SlotIndex::decode(0),
            ctx.scheduler,
            ctx.router,
            ctx.address_spaces,
        )
        .expect("selftest spawn")
        .as_index()
}

/// A supervisor task at `entry` (kernel text), resumed at once — the selftests' lifecycle
/// children. Only the kernel starts a task without an address space of its own.
pub(super) fn spawn_kernel_task(ctx: &mut api::Context<'_>, entry: usize) -> Option<usize> {
    let parent = ctx.tasks.current_pid();
    let entry = VirtAddr::instr_aligned(entry)?;
    let pid = ctx
        .tasks
        .spawn(
            parent,
            entry,
            None,
            None,
            0,
            SlotIndex::decode(0),
            ctx.scheduler,
            ctx.router,
            ctx.address_spaces,
        )
        .ok()?;
    let _ = ctx.tasks.resume_task(pid, ctx.scheduler);
    Some(pid.as_index())
}
