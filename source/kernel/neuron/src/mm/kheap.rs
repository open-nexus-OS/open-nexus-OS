// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The kernel heap's reserve (ADR-0091). An allocation whose size a caller chose — a
//! lazy object's page words, an object's block list, a new task — may not eat into the last
//! `RESERVE` bytes of the kernel heap; such a size is `ENOMEM` to that caller, never an
//! `ALLOC-FAIL` panic in whatever kernel path allocates next.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host unit tests (`room_admits`), `test_reject_*` in syscall/api/reject_tests.rs
//! ADR: docs/adr/0091-no-ambient-authority-over-address-spaces-a-task-maps-and-spawns-only-into-its-own-the-kernel-bounds-what-a-caller-sizes.md

/// Kernel heap bytes kept for the kernel's own bookkeeping.
pub(crate) const RESERVE: usize = 1024 * 1024;

/// Whether `bytes` more fit on a heap with `free` free bytes, above [`RESERVE`].
pub(crate) const fn room_admits(free: usize, bytes: usize) -> bool {
    match free.checked_sub(RESERVE) {
        Some(room) => bytes <= room,
        None => false,
    }
}

/// Whether the kernel heap can give `bytes` to a caller-sized allocation now. A plain lock and
/// metadata read; the allocation itself stays fallible (`try_reserve`), since free bytes are
/// not one free block.
#[cfg(target_os = "none")]
pub(crate) fn admits(bytes: usize) -> bool {
    room_admits(crate::heap_free_bytes(), bytes)
}

/// Host builds run on the host's allocator: the target heap's reserve never binds there.
#[cfg(not(target_os = "none"))]
pub(crate) fn admits(bytes: usize) -> bool {
    room_admits(usize::MAX, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_an_allocation_that_eats_into_the_reserve() {
        assert!(!room_admits(RESERVE - 1, 0), "a heap already inside its reserve admits nothing");
        assert!(!room_admits(RESERVE + 4096, 4097), "one byte into the reserve is refused");
        assert!(room_admits(RESERVE + 4096, 4096), "up to the reserve is admitted");
        assert!(room_admits(RESERVE, 0));
        assert!(!room_admits(0, usize::MAX));
    }
}
