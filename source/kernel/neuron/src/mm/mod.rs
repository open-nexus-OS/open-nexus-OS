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
// 224MB (2026-07-22, was 160MB since 2026-07-06, 96MB before): the pool feeds
// EVERY service image + stack + VMO (framebuffer ~20MB, GL backings, per-app
// surfaces per ADR-0037) and is bump-first with a bounded free list. 96MB sat
// ~1KB from exhaustion once the DSL runtime linked into windowd — and
// exhaustion at spawn time kills a service SILENTLY (see TASK-0076B ledger).
// The DSL app runtime (TASK-0080D) adds a process image + surface VMO PER APP;
// the baked CJK atlases (RFC-0075 Phase 8d) add ~4.5MB to EVERY app-host
// instance, and a logged-in session exhausted 160MB (peak 0x9e0b000 with a
// 4MB surface pending). Machine RAM is 320M (qemu-launcher): the
// identity-mapped arena now ends at 0x9180_0000, leaving 40MB above it.
// Growth discipline still applies: windowd's image size is CI-gated
// (`just contract-windowd-size`), dead one-shot VMOs get freed (#124), and
// sharing ONE atlas via RO VMO (recorded follow-up) claws the per-instance
// duplication back — the pool is headroom, not an excuse.
pub const USER_VMO_ARENA_LEN: usize = 224 * 1024 * 1024;
/// Base address of the kernel-managed user VMO arena. Moved up 0x8180_0000
/// → 0x8280_0000 (RFC-0075 Phase 8d): the kernel image embeds init-lite,
/// which now carries the baked CJK glyph atlases (~24 MB total image) —
/// the fixed windows must sit BEHIND the image end. Arena end = 0x9180_0000
/// (machine RAM is 320 MB → 0x9400_0000; 40 MB stays above).
pub const USER_VMO_ARENA_BASE: usize = 0x8380_0000;
/// Base address of the temporary kernel page-pool window used by early
/// loaders/selftests. Moved 0x80c0_0000 → 0x8200_0000 (behind the grown
/// kernel+init image; see the arena note above).
pub const KERNEL_PAGE_POOL_BASE: usize = 0x8200_0000;
/// Size of the temporary kernel page-pool window. 8 MB → 24 MB: the init
/// loader allocates the WHOLE embedded init image (now ~16.4 MB with the
/// CJK atlases) plus stacks from this pool.
pub const KERNEL_PAGE_POOL_LEN: usize = 24 * 1024 * 1024;
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
