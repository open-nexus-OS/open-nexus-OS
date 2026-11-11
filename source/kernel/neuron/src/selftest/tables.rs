// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot-time proof of ADR-0093 (TASK-0352 P5–P7) on the real kernel heap and pool —
//! the tables of the memory model grow, bounded by the pool, never by a constant. The heap grows
//! by an arena when a caller-sized ask finds it short (the arena stays: it is the heap's working
//! room from here on). A lazy object of 512 MiB reserved costs the heap its chunk table and a
//! touched page one chunk (the frame comes from the pool and goes back). A region table holds
//! its 65th record, a capability table its 257th, an endpoint table its 385th — each past the
//! constant that used to refuse it (64, 256, 384: the last bit at the 29th app). A child of
//! `pressure` (the selftest root sits at its module-size ratchet).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: kheap grows ok (…)`, `KSELFTEST: lazy sparse ok (…)`,
//!   `KSELFTEST: tables grow ok (…)`, `KSELFTEST: vm window ok (…)`, every QEMU lane and the
//!   board ladder

use crate::cap::{CapTable, Capability, CapabilityKind, Held, Rights};
use crate::ipc::Router;
use crate::lazy::LazyPages;
use crate::mm::{frame_pool, PageFlags};
use crate::selftest::Context;
use crate::va_space::{RegionKind, VaRegion, VaSpace};

const PAGE: usize = 4096;
/// The object reserved: 512 MiB, inside what the pool was ever given on the smallest lane.
const RESERVED_PAGES: usize = 1 << 17;
/// The heap a reservation may cost: well above the 1 KiB chunk table of 512 MiB, far below the
/// 512 KiB a word per page used to cost.
const RESERVATION_BUDGET: usize = 64 * 1024;
/// One chunk of 1024 words, and the allocator's own header.
const TOUCH_BUDGET: usize = 4096 + 64;

pub(in crate::selftest) fn run_tables_selftests(ctx: &mut Context<'_>) {
    kheap_grows();
    lazy_sparse();
    tables_grow(ctx);
    vm_window(ctx);
}

/// The whole user half (TASK-0352 P9, ADR-0093): the window finds a hole for a reservation no
/// 768 MiB window held — ending past the old window's end — and a page past that end maps and
/// translates in a fresh address space.
fn vm_window(ctx: &mut Context<'_>) {
    /// Where the window used to end: 768 MiB above its base.
    const OLD_END: usize = crate::mm::USER_VM_WINDOW_BASE + 0x3000_0000;
    const RESERVE: usize = 2 << 30;
    let fail = |what: &str, at: usize| log_error!(target: "selftest", "KSELFTEST: vm window FAIL ({} 0x{:x})", what, at);
    let space = VaSpace::new(crate::mm::USER_VM_WINDOW_BASE, crate::mm::USER_VM_WINDOW_LEN);
    let Some(hole) = space.find_hole(RESERVE, PAGE, 0) else {
        return fail("no hole for", RESERVE);
    };
    if hole + RESERVE <= OLD_END || hole + RESERVE > crate::mm::USER_VADDR_LIMIT {
        return fail("hole", hole);
    }
    let Ok(handle) = ctx.address_spaces.create() else {
        return fail("address space", 0);
    };
    let Ok(block) = frame_pool::alloc(0) else {
        let _ = ctx.address_spaces.destroy(handle);
        return fail("frame", 0);
    };
    let va = OLD_END + PAGE;
    // A physical address fits `usize` on this 64-bit target: the page table speaks `usize`.
    let pa = block.base as usize;
    let flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;
    let spaces = &mut *ctx.address_spaces;
    let mapped = spaces.map_page(handle, va, pa, flags).is_ok()
        && spaces.get(handle).is_ok_and(|s| s.page_table().translate(va) == Some(pa))
        && spaces.unmap_leaf(handle, va) == Ok(PAGE);
    frame_pool::free(block);
    let _ = ctx.address_spaces.destroy(handle);
    if mapped {
        log_info!(
            target: "selftest",
            "KSELFTEST: vm window ok (len=0x{:x} reserved=0x{:x} at=0x{:x} mapped=0x{:x})",
            crate::mm::USER_VM_WINDOW_LEN,
            RESERVE,
            hole,
            va
        );
    } else {
        fail("map", va);
    }
}

/// An ask larger than everything the heap has free: it must grow for it, by one arena.
fn kheap_grows() {
    let (used, free, arenas) =
        (crate::kheap::used_bytes(), crate::kheap::free_bytes(), crate::kheap::arena_count());
    let admitted = crate::kheap::admits(free + 1);
    let (used_after, free_after, arenas_after) =
        (crate::kheap::used_bytes(), crate::kheap::free_bytes(), crate::kheap::arena_count());
    let (from, to) = (used + free, used_after + free_after);
    if admitted && to > from && arenas_after == arenas + 1 {
        log_info!(
            target: "selftest",
            "KSELFTEST: kheap grows ok (from={} to={} arenas={} asked={})",
            from,
            to,
            arenas_after,
            free + 1
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: kheap grows FAIL (admitted={} from={} to={} arenas={}->{})",
            admitted,
            from,
            to,
            arenas,
            arenas_after
        );
    }
}

/// Reserving costs the chunk table, a touch one chunk, and the chunk goes when its page does.
fn lazy_sparse() {
    let fail = |step: &str, bytes: usize| log_error!(target: "selftest", "KSELFTEST: lazy sparse FAIL ({} bytes={})", step, bytes);
    let Some(mut pages) = LazyPages::try_new(RESERVED_PAGES) else {
        return fail("reserve", 0);
    };
    let reserved = pages.heap_bytes();
    if reserved > RESERVATION_BUDGET {
        return fail("reserved", reserved);
    }
    let Ok(block) = frame_pool::alloc(0) else {
        return fail("frame", 0);
    };
    let touched = match pages.commit(5, block.base) {
        Ok(()) => pages.heap_bytes(),
        Err(_) => {
            frame_pool::free(block);
            return fail("commit", reserved);
        }
    };
    let back = pages.uncommit(5) == Ok(block.base);
    let after = pages.heap_bytes();
    frame_pool::free(block);
    if !back {
        return fail("uncommit", after);
    }
    if touched <= reserved || touched - reserved > TOUCH_BUDGET || after != reserved {
        return fail("touched", touched);
    }
    log_info!(
        target: "selftest",
        "KSELFTEST: lazy sparse ok (reserved={} heap_reserved={} heap_touched={} heap_after={})",
        RESERVED_PAGES * PAGE,
        reserved,
        touched,
        after
    );
}

/// A region table, a capability table and an endpoint table, each one past its old constant.
fn tables_grow(_ctx: &mut Context<'_>) {
    let fail = |table: &str, at: usize| log_error!(target: "selftest", "KSELFTEST: tables grow FAIL ({} at={})", table, at);
    let mut space = VaSpace::new(crate::mm::USER_VM_WINDOW_BASE, crate::mm::USER_VM_WINDOW_LEN);
    for i in 0..65 {
        // A record in a local table, never mapped: the frame address is a placeholder (the
        // kernel's own first page), so the table's growth is what this proves.
        let region = VaRegion {
            va: crate::mm::USER_VM_WINDOW_BASE + i * 2 * PAGE,
            len: PAGE,
            pa: i * PAGE,
            flags: 0,
            kind: RegionKind::Vmo,
            vmo: 1,
            offset: 0,
            lazy: false,
        };
        if space.insert(region).is_err() {
            return fail("regions", i);
        }
    }
    let regions = space.region_count();
    let mut caps = CapTable::new();
    let cap = Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE };
    for i in 0..257 {
        if Held::plain(cap).and_then(|held| caps.allocate(held)) != Ok(i) {
            return fail("caps", i);
        }
    }
    let mut local = Router::new(0);
    for i in 0..385 {
        if local.create_endpoint(1, None).is_err() {
            return fail("endpoints", i);
        }
    }
    let endpoints = local.live_endpoint_count();
    if regions != 65 || caps.capacity() < 257 || endpoints != 385 {
        return fail("counts", regions + caps.capacity() + endpoints);
    }
    log_info!(
        target: "selftest",
        "KSELFTEST: tables grow ok (regions={} caps=257 cap_slots={} endpoints={})",
        regions,
        caps.capacity(),
        endpoints
    );
}
