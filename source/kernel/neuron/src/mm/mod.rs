// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Virtual memory primitives for Sv39 address spaces. The kernel half
//! is a direct map in the high half (`crate::phys`, RFC-0098 C4); physical
//! memory is the frame pool's (`frame_pool`), VMOs are page-backed objects
//! (`vmo`), the devices a DMA master's memory must reach are `devices`, and no
//! physical address constant remains — the only window below is the user VA
//! window of RFC-0085.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: QEMU selftests + boot markers
//! PUBLIC API: address_space::{AddressSpaceManager, AsHandle}, page_table::{PageTable, PageFlags}
//! DEPENDS_ON: arch::riscv, hal::platform (for logging), core alloc
//! INVARIANTS: W^X policy; canonical Sv39 ranges; stable PAGE_SIZE
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

pub mod address_space;
mod asid;
pub mod page_table;
pub mod reclaim;
pub mod vm_ops;

pub use address_space::{AddressSpaceError, AddressSpaceManager, AsHandle};
pub use page_table::{MapError, PageFlags, PAGE_SIZE};

/// The top of the Sv39 user half: no user address — a pointer a syscall checks, a mapping the
/// window hands out — reaches it. Every user VA is below 2^38, sign-positive, and can never read
/// as a -errno.
pub const USER_VADDR_LIMIT: usize = 0x40_0000_0000;

/// RFC-0085: the kernel-managed user mapping window. `vm_map`/`mmio_map_auto`
/// allocate ONLY here; fixed-VA maps into it are refused (EPERM) — that
/// invariant is what keeps the `va_space` hole-finder sound without ever
/// consulting the page table. Bounds: above every inventoried fixed-VA use
/// (gpud's legacy carve-outs end at 0x4400_0000; 0x4400_0000..0x5000_0000
/// stays a reserved guard band) up to `USER_VADDR_LIMIT` — the whole user
/// half (TASK-0352 P9, ADR-0093: the window was 768 MiB to 0x8000_0000; a
/// reservation is sparse and costs no page, so the hole-finder's space is the
/// address space's, not a budget's).
pub const USER_VM_WINDOW_BASE: usize = 0x5000_0000;
pub const USER_VM_WINDOW_LEN: usize = USER_VADDR_LIMIT - USER_VM_WINDOW_BASE;

pub mod devices;
pub mod frame_pool;
mod kernel_layout;
#[cfg(test)]
mod page_table_tests;
mod page_table_verify;
pub mod populate;
pub mod pressure_endpoint;
pub mod squeeze;
pub mod swap;
mod tests;
pub mod usage;
pub mod vmo;
