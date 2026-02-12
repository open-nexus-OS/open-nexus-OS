// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Virtual memory primitives for Sv39 address spaces
//! OWNERS: @kernel-mm-team
//! PUBLIC API: address_space::{AddressSpaceManager, AsHandle}, page_table::{PageTable, PageFlags}
//! DEPENDS_ON: arch::riscv, hal::virt (for logging), core alloc
//! INVARIANTS: W^X policy; canonical Sv39 ranges; stable PAGE_SIZE
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

pub mod address_space;
pub mod page_table;

pub use address_space::{AddressSpaceError, AddressSpaceManager, AsHandle};
pub use page_table::{MapError, PageFlags, PAGE_SIZE};

/// Size reserved for user VMO allocations directly managed by the kernel.
pub const USER_VMO_ARENA_LEN: usize = 16 * 1024 * 1024;
/// Base address of the kernel-managed user VMO arena.
pub const USER_VMO_ARENA_BASE: usize = 0x8100_0000;

#[cfg(test)]
mod tests;
