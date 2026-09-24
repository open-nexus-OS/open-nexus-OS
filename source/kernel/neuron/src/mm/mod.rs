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
pub mod page_table;
pub mod vm_ops;

pub use address_space::{AddressSpaceError, AddressSpaceManager, AsHandle};
pub use page_table::{MapError, PageFlags, PAGE_SIZE};

/// RFC-0085: the kernel-managed user mapping window. `vm_map`/`mmio_map_auto`
/// allocate ONLY here; fixed-VA maps into it are refused (EPERM) — that
/// invariant is what keeps the `va_space` hole-finder sound without ever
/// consulting the page table. Bounds: above every inventoried fixed-VA use
/// (gpud's legacy carve-outs end at 0x4400_0000; 0x4400_0000..0x5000_0000
/// stays a reserved guard band) and below `USER_VADDR_LIMIT` (0x8000_0000),
/// so every returned VA is sign-positive and can never read as a -errno.
pub const USER_VM_WINDOW_BASE: usize = 0x5000_0000;
pub const USER_VM_WINDOW_LEN: usize = 0x3000_0000; // 768 MiB

pub mod devices;
pub mod frame_pool;
mod kernel_layout;
#[cfg(test)]
mod page_table_tests;
mod page_table_verify;
mod tests;
pub mod usage;
pub mod vmo;
