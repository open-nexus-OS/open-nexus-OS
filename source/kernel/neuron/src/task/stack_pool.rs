// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the legacy guarded user-stack allocator for the non-exec `spawn`
//! path (bootstrap tasks). It hands out fixed 4-page stacks from a small
//! identity-mapped kernel window (0x8010_0000..0x8020_0000) with a guard page.
//! The `exec` loaders map their stacks from the VMO arena instead (see
//! `syscall::api::exec::map_process_stack`); this pool serves only kernel-side
//! spawns. Split out of `task/mod.rs` (RFC-0075 8e, module-size ratchet).
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU spawn markers (bootstrap task stacks)
//! INVARIANTS: cursor stays within [STACK_POOL_BASE, STACK_POOL_LIMIT]; a
//!   corrupt/uninitialized cursor is re-seeded loudly, never used blind.

use super::SpawnError;
use crate::mm::{AddressSpaceManager, AsHandle, PageFlags, PAGE_SIZE};
use crate::types::VirtAddr;
use spin::Mutex;

const USER_STACK_TOP: usize = 0x4000_0000;
const STACK_PAGES: usize = 4;
const STACK_POOL_BASE: usize = 0x8000_0000 + 0x10_0000;
const STACK_POOL_LIMIT: usize = 0x8000_0000 + 0x20_0000;

struct StackPool {
    cursor: usize,
}

impl StackPool {
    const fn new() -> Self {
        Self { cursor: STACK_POOL_LIMIT }
    }

    fn alloc(&mut self, pages: usize) -> Option<usize> {
        // Robust bring-up: if `.data` initializers are unavailable (or if this static lives in
        // a NOLOAD region), `cursor` may be zero. Treat zero as "uninitialized" and seed it from
        // the compile-time limit.
        if self.cursor == 0 {
            self.cursor = STACK_POOL_LIMIT;
        }
        // Integrity gate (P0.1 layout audit): a cursor OUTSIDE the pool window
        // means the `.data` initializer was corrupted/mis-loaded — say the
        // VALUE loudly (the value fingerprints the writer) instead of failing
        // as an anonymous StackExhausted at some later spawn.
        if self.cursor < STACK_POOL_BASE || self.cursor > STACK_POOL_LIMIT {
            log_error!(
                "STACK-POOL cursor corrupt: 0x{:x} (window 0x{:x}..0x{:x}) — image/.data integrity",
                self.cursor,
                STACK_POOL_BASE,
                STACK_POOL_LIMIT
            );
            self.cursor = STACK_POOL_LIMIT;
        }
        let bytes = pages.checked_mul(PAGE_SIZE)?;
        let next = self.cursor.checked_sub(bytes)?;
        if next < STACK_POOL_BASE {
            log_error!(
                "STACK-POOL exhausted: cursor=0x{:x} want={} pages (window 0x{:x}..0x{:x})",
                self.cursor,
                pages,
                STACK_POOL_BASE,
                STACK_POOL_LIMIT
            );
            None
        } else {
            self.cursor = next;
            Some(next)
        }
    }
}

static STACK_ALLOCATOR: Mutex<StackPool> = Mutex::new(StackPool::new());

pub(super) fn allocate_guarded_stack(
    address_spaces: &mut AddressSpaceManager,
    handle: AsHandle,
) -> Result<VirtAddr, SpawnError> {
    let phys_base = {
        let mut pool = STACK_ALLOCATOR.lock();
        pool.alloc(STACK_PAGES).ok_or(SpawnError::StackExhausted)?
    };
    // RFC-0004: zero newly allocated stack pages so no stale bytes leak into user space.
    // This relies on the kernel identity-mapping `STACK_POOL_BASE..STACK_POOL_LIMIT`.
    unsafe {
        core::ptr::write_bytes(phys_base as *mut u8, 0, STACK_PAGES * PAGE_SIZE);
    }
    let flags = PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::USER;
    let guard_bottom = USER_STACK_TOP - (STACK_PAGES + 1) * PAGE_SIZE;
    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(
            u,
            "STACK: base=0x{:x} guard_bottom=0x{:x} pages={}\n",
            phys_base, guard_bottom, STACK_PAGES
        );
    }
    for page in 0..STACK_PAGES {
        let page_va = guard_bottom + PAGE_SIZE + page * PAGE_SIZE;
        let page_pa = phys_base + page * PAGE_SIZE;
        address_spaces.map_page(handle, page_va, page_pa, flags)?;
        #[cfg(feature = "debug_uart")]
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "STACK: map idx={} va=0x{:x} pa=0x{:x}\n", page, page_va, page_pa);
        }
    }
    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(u, "STACK: top=0x{:x}\n", USER_STACK_TOP);
    }
    VirtAddr::page_aligned(USER_STACK_TOP).ok_or(SpawnError::InvalidStackPointer)
}
