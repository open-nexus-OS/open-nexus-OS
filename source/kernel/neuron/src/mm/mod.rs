// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Virtual memory primitives for Sv39 address spaces. The kernel half
//! is a direct map in the high half (`crate::phys`, RFC-0098 C4); VMOs are
//! page-backed objects (`vmo`), and the address constants below are the LAST
//! fixed physical windows (TASK-0286 P3b deletes them), reachable only through
//! `phys_to_virt`.
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

pub const KERNEL_PAGE_POOL_BASE: usize = 0x8200_0000;
/// Size of the temporary kernel page-pool window. 8 MB → 24 MB: the init
/// loader allocates the WHOLE embedded init image (now ~16.4 MB with the
/// CJK atlases) plus stacks from this pool.
pub const KERNEL_PAGE_POOL_LEN: usize = 24 * 1024 * 1024;
/// The bootstrap task's "identity VMO" `(base, len)` (cap slot 1, `kmain`):
/// the last fixed physical window a capability names outright — P3 retires it.
pub const BOOTSTRAP_IDENTITY_WINDOW: (usize, usize) = (0x8000_0000, 0x10_0000);
pub mod frame_pool;
mod kernel_layout;
#[cfg(test)]
mod page_table_tests;
mod page_table_verify;
mod tests;
pub mod vmo;
