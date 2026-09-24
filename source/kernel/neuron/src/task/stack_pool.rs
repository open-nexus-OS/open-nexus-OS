// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the guarded user stack for the non-exec `spawn` path (bootstrap
//! tasks): four pages as ONE frame-pool block below a guard page (TASK-0286
//! P3b — the fixed stack window is gone). The `exec` loaders build their
//! stacks from pool blocks too (`syscall::api::exec::map_process_stack`);
//! this path serves only kernel-side spawns. Split out of `task/mod.rs`
//! (RFC-0075 8e, module-size ratchet).
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU spawn markers (bootstrap task stacks)
//! INVARIANTS: the block is recorded on the task and returned with its image;
//!   a pool refusal is `StackExhausted`, never a silent fallback.

use super::SpawnError;
use crate::frames::Block;
use crate::mm::{AddressSpaceManager, AsHandle, PageFlags, PAGE_SIZE};
use crate::types::VirtAddr;

const USER_STACK_TOP: usize = 0x4000_0000;
const STACK_PAGES: usize = 4;
/// The stack's frames come from the pool as ONE block (TASK-0286 P3b) and
/// are returned when the task's image is released: the block rides on the
/// task's `ImageAllocs` like every other page of its image.
pub(super) fn allocate_guarded_stack(
    address_spaces: &mut AddressSpaceManager,
    handle: AsHandle,
) -> Result<(VirtAddr, Block), SpawnError> {
    const STACK_ORDER: u8 = STACK_PAGES.trailing_zeros() as u8;
    const _: () = assert!(STACK_PAGES.is_power_of_two());
    let block =
        crate::mm::frame_pool::alloc(STACK_ORDER).map_err(|_| SpawnError::StackExhausted)?;
    let phys_base = block.base as usize;
    // RFC-0004: zero newly allocated stack pages so no stale bytes leak into
    // user space — through the direct map.
    unsafe {
        core::ptr::write_bytes(
            crate::phys::phys_to_virt(phys_base) as *mut u8,
            0,
            STACK_PAGES * PAGE_SIZE,
        );
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
    let top = VirtAddr::page_aligned(USER_STACK_TOP).ok_or(SpawnError::InvalidStackPointer)?;
    Ok((top, block))
}
