// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Virtual memory primitives for Sv39 address spaces
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: QEMU selftests + boot markers
//! PUBLIC API: address_space::{AddressSpaceManager, AsHandle}, page_table::{PageTable, PageFlags}
//! DEPENDS_ON: arch::riscv, hal::virt (for logging), core alloc
//! INVARIANTS: W^X policy; canonical Sv39 ranges; stable PAGE_SIZE
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

pub mod address_space;
pub mod page_table;

pub use address_space::{AddressSpaceError, AddressSpaceManager, AsHandle};
pub use page_table::{MapError, PageFlags, PAGE_SIZE};

/// Size reserved for user VMO allocations directly managed by the kernel.
///
/// The live interactive UI lane needs enough headroom for the full ramfb-sized
/// framebuffer VMO after normal service bring-up has already allocated virtio,
/// exec, metadata, and proof buffers.
// 96MB: the pool is bump-only (never frees), and 64MB was exactly exhausted the
// moment one more service (sessiond) loaded its ELF+stack — gpud's late 4MB GL
// backings then failed with resource-exhausted and the GL compositor silently
// fell back to 2D. Machine RAM is 320M (qemu-launcher), so the identity-mapped
// arena ending at 0x8780_0000 leaves ample headroom. Follow-up hygiene: free
// dead one-shot VMOs (the 4MB bootstrap-splash resource) instead of growing.
pub const USER_VMO_ARENA_LEN: usize = 96 * 1024 * 1024;
/// Base address of the kernel-managed user VMO arena.
pub const USER_VMO_ARENA_BASE: usize = 0x8180_0000;
/// Base address of the temporary kernel page-pool window used by early loaders/selftests.
pub const KERNEL_PAGE_POOL_BASE: usize = 0x80c0_0000;
/// Size of the temporary kernel page-pool window.
pub const KERNEL_PAGE_POOL_LEN: usize = 8 * 1024 * 1024;
/// Typed memory window descriptor used to avoid base/length mixups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressWindow {
    /// Window base address.
    pub base: usize,
    /// Window length in bytes.
    pub len: usize,
}

impl AddressWindow {
    /// Returns the exclusive window end.
    #[must_use]
    pub const fn end(self) -> usize {
        self.base + self.len
    }
}

/// Temporary kernel page-pool window used by early loader and selftest allocators.
pub const KERNEL_PAGE_POOL_WINDOW: AddressWindow =
    AddressWindow { base: KERNEL_PAGE_POOL_BASE, len: KERNEL_PAGE_POOL_LEN };

#[cfg(test)]
mod tests;
