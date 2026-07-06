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
// 160MB (2026-07-06, was 96MB): the pool feeds EVERY service image + stack +
// VMO (framebuffer ~20MB, GL backings, per-app surfaces per ADR-0037) and is
// bump-first with a bounded free list. 96MB sat ~1KB from exhaustion once the
// DSL runtime linked into windowd — and exhaustion at spawn time kills a
// service SILENTLY (see TASK-0076B ledger). The DSL app runtime (TASK-0080D)
// adds a process image + surface VMO PER APP, so the budget must carry a
// desktop's worth of apps, not one compositor. Machine RAM is 320M
// (qemu-launcher): the identity-mapped arena now ends at 0x8B80_0000, leaving
// >130MB above it. Growth discipline still applies: windowd's image size is
// CI-gated (`just contract-windowd-size`) and dead one-shot VMOs get freed
// (#124) — the pool is headroom, not an excuse.
pub const USER_VMO_ARENA_LEN: usize = 160 * 1024 * 1024;
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
