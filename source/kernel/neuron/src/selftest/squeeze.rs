// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot-time proof of the squeeze (TASK-0333 6f-1, ADR-0087, RFC-0100 §5) against the
//! real syscall table and the fault's own fill. A parent and its child (selftest tasks); the
//! child's space holds three lazy objects — its own (8 pages: 4 of text, 1 of zeroes, 1 of
//! noise, 2 never touched), one another space maps too, one a device was told of. The refusals
//! first: a stranger's order is `ESRCH`, the order for a child that still runs is `EBUSY`. Then
//! the freeze and the batches until `0`: the text pages recorded, the zero page dropped, the
//! noise page kept, the two foreign objects untouched, the PTEs of the taken pages gone, the
//! stores holding exactly the records. Every page is then touched through `populate::fill` — the
//! user fault's path: the text back byte-exact, the zero page zero, the noise page as it was, the
//! store empty and its frames back. Last a second squeeze and the object's death: the records go
//! with it. A child of `pressure` (the selftest root sits at its module-size ratchet).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: task squeeze ok (…)`, every QEMU lane and the board ladder

use crate::lazy::Access;
use crate::mm::{populate, vmo, AddressSpaceManager, AsHandle, PageFlags, PAGE_SIZE};
use crate::selftest::Context;
use crate::syscall::{
    api, Args, Error as SysError, SyscallTable, SYSCALL_TASK_SQUEEZE, SYSCALL_TASK_SUSPEND,
};
use crate::task::Pid;

/// The child's own object: 8 pages at this address (below the managed window: kernel-placed).
const OWN_VA: usize = 0x0010_0000;
const SHARED_VA: usize = 0x0020_0000;
const DEVICE_VA: usize = 0x0030_0000;
/// The parent's record page.
const OUT_VA: usize = 0x1000;
const PAGES: usize = 8;
/// Pages `0..TEXT` hold text, page `TEXT` zeroes, page `TEXT + 1` noise; the rest stay untouched.
const TEXT: usize = 4;
const NOISE: usize = TEXT + 1;

/// The byte at `at` of the own object's page `page`.
fn pattern(page: usize, at: usize) -> u8 {
    const PHRASE: &[u8] =
        b"the parent orders, the child is parked, the pool takes its frames back. ";
    match page {
        p if p < TEXT => PHRASE[(at + p * 7) % PHRASE.len()],
        TEXT => 0,
        _ => {
            // A hash per byte: no four bytes repeat, the page does not compress.
            let mut x = ((page as u64) << 32) | at as u64 | 1;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
        }
    }
}

/// The bytes of committed page `page` of `id`, through the direct map.
fn page_bytes(id: u32, page: usize) -> Option<&'static mut [u8]> {
    let pa = vmo::translate(id, page * PAGE_SIZE)?;
    // SAFETY: a committed frame of an object this selftest made and alone uses; the slice is
    // dropped before the frame can go back to the pool.
    Some(unsafe {
        core::slice::from_raw_parts_mut(crate::phys::phys_to_virt(pa) as *mut u8, PAGE_SIZE)
    })
}

fn record(
    spaces: &mut AddressSpaceManager,
    handle: AsHandle,
    va: usize,
    len: usize,
    id: u32,
) -> bool {
    let rw = (PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE).bits();
    spaces.get_mut(handle).is_ok_and(|s| s.va_space_mut().record_lazy(va, len, rw, id).is_ok())
}

fn order(
    table: &SyscallTable,
    sys: &mut api::Context<'_>,
    number: usize,
    pid: Pid,
    from: usize,
) -> Result<usize, SysError> {
    let args =
        Args::new([pid.as_raw() as usize, from, OUT_VA, crate::mm::squeeze::BATCH_BYTES, 0, 0]);
    table.dispatch(number, sys, &args)
}

/// The record the last batch wrote into the parent's page: `[squeezed, zero, kept, taken, stored]`.
fn batch(out: u32) -> Option<[u64; 5]> {
    let bytes = page_bytes(out, 0)?;
    let word =
        |i: usize| u64::from_le_bytes(bytes[8 + i * 8..16 + i * 8].try_into().unwrap_or([0; 8]));
    let header = u32::from_le_bytes(bytes[0..4].try_into().unwrap_or([0; 4]));
    (header == crate::mm::squeeze::BATCH_VERSION)
        .then(|| [word(0), word(1), word(2), word(3), word(4)])
}

pub(in crate::selftest) fn run_squeeze_selftest(ctx: &mut Context<'_>) {
    let fail =
        |step: &str| log_error!(target: "selftest", "KSELFTEST: task squeeze FAIL ({})", step);
    ctx.tasks.set_current(Pid::KERNEL);
    let parent = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let child = ctx.tasks.selftest_create_dummy_task(parent, ctx.scheduler);
    let stranger = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let reach = crate::dma_reach::DmaReach::ALL;
    let lazy = |len| vmo::create(len, vmo::VmoKind::Lazy, &reach);
    let (Ok(own), Ok(shared), Ok(device), Ok(out)) =
        (lazy(PAGES * PAGE_SIZE), lazy(PAGE_SIZE), lazy(PAGE_SIZE), lazy(PAGE_SIZE))
    else {
        return fail("create");
    };
    let spaces = &mut *ctx.address_spaces;
    let (Ok(child_as), Ok(parent_as), Ok(other_as)) =
        (spaces.create(), spaces.create(), spaces.create())
    else {
        return fail("space");
    };
    let wired = record(spaces, child_as, OWN_VA, PAGES * PAGE_SIZE, own)
        && record(spaces, child_as, SHARED_VA, PAGE_SIZE, shared)
        && record(spaces, child_as, DEVICE_VA, PAGE_SIZE, device)
        && record(spaces, other_as, SHARED_VA, PAGE_SIZE, shared)
        && record(spaces, parent_as, OUT_VA, PAGE_SIZE, out)
        && spaces.attach(child_as, child).is_ok()
        && spaces.attach(parent_as, parent).is_ok();
    for (pid, handle) in [(child, child_as), (parent, parent_as)] {
        if let Some(task) = ctx.tasks.task_mut(pid) {
            task.address_space = Some(handle);
        }
    }
    vmo::mark_device_named(device);
    // The child's pages committed and mapped by the fault's own fill, then written.
    let filled = ctx.address_spaces.get_mut(child_as).is_ok_and(|space| {
        (0..=NOISE).all(|i| populate::fill(space, OWN_VA + i * PAGE_SIZE, Access::Write).is_ok())
            && populate::fill(space, SHARED_VA, Access::Write).is_ok()
            && populate::fill(space, DEVICE_VA, Access::Write).is_ok()
    });
    let mut written = filled;
    for i in 0..=NOISE {
        match page_bytes(own, i) {
            Some(bytes) => bytes.iter_mut().enumerate().for_each(|(at, b)| *b = pattern(i, at)),
            None => written = false,
        }
    }
    for id in [shared, device] {
        match page_bytes(id, 0) {
            Some(bytes) => bytes.iter_mut().enumerate().for_each(|(at, b)| *b = pattern(0, at)),
            None => written = false,
        }
    }

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
    let sys = &mut sys_ctx;

    // The refusals: a stranger's order, the parent's while the child still runs.
    sys.tasks.set_current(stranger);
    let stranger_refused =
        order(&table, sys, SYSCALL_TASK_SQUEEZE, child, 0) == Err(SysError::InvalidTarget);
    sys.tasks.set_current(parent);
    let running_refused =
        order(&table, sys, SYSCALL_TASK_SQUEEZE, child, 0) == Err(SysError::ResourceBusy);
    let frozen = order(&table, sys, SYSCALL_TASK_SUSPEND, child, 0) == Ok(1);

    // The batches until `0`.
    let (base_pages, _, base_frames) = vmo::squeeze::stats();
    let mut totals = [0u64; 5];
    let (mut from, mut calls) = (0usize, 0usize);
    let done = loop {
        calls += 1;
        let Ok(next) = order(&table, sys, SYSCALL_TASK_SQUEEZE, child, from) else { break false };
        let Some(record) = batch(out) else { break false };
        for (total, value) in totals.iter_mut().zip(record) {
            *total += value;
        }
        if next == 0 {
            break true;
        }
        if next <= from || calls > 64 {
            break false;
        }
        from = next;
    };
    let [squeezed, zero, kept, taken, stored] = totals;
    let counted = squeezed == TEXT as u64 && zero == 1 && kept == 1 && taken >= 1 && stored > 0;
    let (held_pages, _, held_frames) = vmo::squeeze::stats();
    let held = held_pages == base_pages + TEXT as u64 && held_frames == base_frames + taken;
    let resident = vmo::committed_in(own, 0, PAGES * PAGE_SIZE) == PAGE_SIZE;
    let untouched = vmo::committed_in(shared, 0, PAGE_SIZE) == PAGE_SIZE
        && vmo::committed_in(device, 0, PAGE_SIZE) == PAGE_SIZE;
    let mapped = |space: &crate::mm::address_space::AddressSpace, i: usize| {
        space
            .page_table()
            .leaf_flags(OWN_VA + i * PAGE_SIZE)
            .is_ok_and(|f| f.contains(PageFlags::VALID))
    };
    let unmapped = sys
        .address_spaces
        .get(child_as)
        .is_ok_and(|space| (0..NOISE).all(|i| !mapped(space, i)) && mapped(space, NOISE));

    // Every page back through the fault's fill: byte-exact, the store empty again.
    let inflated_before = vmo::squeeze::inflated();
    let back = sys.address_spaces.get_mut(child_as).is_ok_and(|space| {
        (0..=NOISE).all(|i| populate::fill(space, OWN_VA + i * PAGE_SIZE, Access::Read).is_ok())
    });
    let exact = (0..=NOISE).all(|i| {
        page_bytes(own, i)
            .is_some_and(|bytes| bytes.iter().enumerate().all(|(at, b)| *b == pattern(i, at)))
    });
    let inflated = vmo::squeeze::inflated() - inflated_before;
    let (after_pages, _, after_frames) = vmo::squeeze::stats();
    let restored = back
        && exact
        && inflated == TEXT as u64
        && after_pages == base_pages
        && after_frames == base_frames;

    // A second squeeze, then the object's death: the records go with it.
    let again = order(&table, sys, SYSCALL_TASK_SQUEEZE, child, 0) == Ok(0)
        && vmo::squeeze::stats().0 == base_pages + TEXT as u64;
    for (handle, pid) in [(child_as, Some(child)), (parent_as, Some(parent)), (other_as, None)] {
        if let Some(pid) = pid {
            let _ = sys.address_spaces.detach(handle, pid);
            if let Some(task) = sys.tasks.task_mut(pid) {
                task.address_space = None;
            }
        }
        let _ = sys.address_spaces.destroy(handle);
    }
    for id in [own, shared, device, out] {
        let _ = vmo::destroy(id);
    }
    let (end_pages, _, end_frames) = vmo::squeeze::stats();
    let died = end_pages == base_pages && end_frames == base_frames;
    sys.tasks.set_current(Pid::KERNEL);

    if stranger_refused
        && running_refused
        && wired
        && written
        && frozen
        && done
        && counted
        && held
        && resident
        && untouched
        && unmapped
        && restored
        && again
        && died
    {
        log_info!(
            target: "selftest",
            "KSELFTEST: task squeeze ok (pages={} zero={} kept={} store={} stored={} inflated={} reject=stranger,running,shared,device)",
            squeezed,
            zero,
            kept,
            taken,
            stored,
            inflated
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: task squeeze FAIL (reject={}{} wired={} written={} frozen={} done={} pages={} zero={} kept={} taken={} held={} resident={} untouched={} unmapped={} restored={} again={} died={})",
            u8::from(stranger_refused),
            u8::from(running_refused),
            wired,
            written,
            frozen,
            done,
            squeezed,
            zero,
            kept,
            taken,
            held,
            resident,
            untouched,
            unmapped,
            restored,
            again,
            died
        );
    }
}
