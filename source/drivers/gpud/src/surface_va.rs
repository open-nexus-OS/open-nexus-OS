// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The address map of windowd's GPU context on the board — pure, host-tested: the
//! context's own buffers in its private GiB (the scratches, the glass cache's planes, the pass
//! memory) and where an app surface object lies (RFC-0114 §1). One file, so no two of them can
//! be given the same addresses unseen (a cache plane placed at the window's start was refused
//! as an overlap with the first app's buffer, `board--2026-11-18T11-44-54`). App objects: from the tail of the context's private GiB (the scratches already open
//! its directory) up to the layout's GiB (TASK-0351 P2, ADR-0093: the windows are bounded by
//! memory, not by this window — a GiB entered costs one directory), one first-fit range per
//! object on a 2 MiB boundary (a page table of its own each), a freed object's range reused.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below incl. `test_reject_a_range_past_the_window`,
//!   `test_reject_a_cache_plane_past_its_range`, `no_two_ranges_of_the_context_overlap`

use alloc::vec::Vec;

/// The context's private GiB: its own buffers, each at a fixed address on a 2 MiB boundary
/// (`backend/gpu/desktop.rs` makes and binds them).
pub(crate) const PRIVATE_VA: u64 = 0x20_0000_0000;
/// The glass cache's further planes: 8 MiB each (a plane is 7.5 MiB) from the GiB's start.
pub(crate) const CACHE_GROWN_VA: u64 = PRIVATE_VA;
pub(crate) const CACHE_PLANE_SPAN: u64 = 8 << 20;
/// The full-resolution scratch, the half-resolution one, the cache's first plane, the frame's
/// pass memory — 32 MiB of addresses each.
pub(crate) const SCRATCH_VA: u64 = PRIVATE_VA + 0x0800_0000;
pub(crate) const HALF_VA: u64 = PRIVATE_VA + 0x0A00_0000;
pub(crate) const CACHE_VA: u64 = PRIVATE_VA + 0x0C00_0000;
pub(crate) const FRAME_VA: u64 = PRIVATE_VA + 0x0E00_0000;
/// Planes the cache's addresses hold: the first and those below the scratch (68 windows' glass).
pub(crate) const CACHE_PLANES: u32 = 1 + ((SCRATCH_VA - CACHE_GROWN_VA) / CACHE_PLANE_SPAN) as u32;

/// Where cache plane `plane` is bound; `None` past the planes' addresses.
pub(crate) const fn cache_plane_va(plane: u32) -> Option<u64> {
    match plane {
        0 => Some(CACHE_VA),
        n if n < CACHE_PLANES => Some(CACHE_GROWN_VA + (n as u64 - 1) * CACHE_PLANE_SPAN),
        _ => None,
    }
}

/// The window: past the frame's own buffer in the private GiB (`backend/gpu/desktop.rs`), to the
/// layout's GiB (`LAYOUT_VA`) — 127 GiB of addresses; the first GiB's tail alone holds some 26
/// full-HD two-slot surfaces, so the 27th window enters the second GiB.
pub(crate) const VA_START: u64 = 0x20_1000_0000;
pub(crate) const VA_END: u64 = 0x40_0000_0000;
/// Each object starts on a page table's boundary.
pub(crate) const VA_ALIGN: u64 = 2 << 20;

/// The first free range of `len` bytes in `[VA_START, VA_END)` beside the `taken` `(va, span)`
/// ones.
pub(crate) fn surface_va(taken: &[(u64, u64)], len: u64) -> Option<u64> {
    let len = len.checked_next_multiple_of(VA_ALIGN)?;
    let mut ranges: Vec<(u64, u64)> = taken.to_vec();
    ranges.sort_unstable();
    let mut at = VA_START;
    for (va, span) in ranges {
        if va >= at.checked_add(len)? {
            break;
        }
        at = at.max(va.saturating_add(span));
    }
    (at.checked_add(len)? <= VA_END).then_some(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_take_first_fit_ranges_on_table_boundaries_inside_the_window() {
        let a = surface_va(&[], 28 << 20).expect("a");
        assert_eq!(a, VA_START);
        let b = surface_va(&[(a, 28 << 20)], 3 << 20).expect("b");
        assert_eq!(b, VA_START + (28 << 20));
        // A hole left by a freed object is reused when it fits.
        let c = surface_va(&[(a, 4 << 20), (a + (8 << 20), 4 << 20)], 4 << 20).expect("c");
        assert_eq!(c, a + (4 << 20));
        assert_eq!(c % VA_ALIGN, 0);
    }

    #[test]
    fn no_two_ranges_of_the_context_overlap() {
        let fixed = 32 << 20;
        let mut ranges: Vec<(u64, u64)> = (0..CACHE_PLANES)
            .map(|p| (cache_plane_va(p).expect("a plane"), CACHE_PLANE_SPAN))
            .chain([(SCRATCH_VA, fixed), (HALF_VA, fixed), (FRAME_VA, fixed)])
            .chain([(VA_START, VA_END - VA_START)])
            .collect();
        ranges.sort_unstable();
        for pair in ranges.windows(2) {
            assert!(pair[0].0 + pair[0].1 <= pair[1].0, "{:#x?}", pair);
        }
        assert!(ranges.iter().all(|(va, _)| va % VA_ALIGN == 0));
        assert_eq!(cache_plane_va(0), Some(0x20_0C00_0000));
        assert_eq!(CACHE_PLANES, 17);
    }

    #[test]
    fn test_reject_a_cache_plane_past_its_range() {
        assert!(
            cache_plane_va(CACHE_PLANES - 1).is_some_and(|va| va + CACHE_PLANE_SPAN <= SCRATCH_VA)
        );
        assert_eq!(cache_plane_va(CACHE_PLANES), None);
        assert_eq!(cache_plane_va(u32::MAX), None);
    }

    #[test]
    fn test_reject_a_range_past_the_window() {
        let window = VA_END - VA_START;
        assert_eq!(surface_va(&[], window + 1), None);
        assert_eq!(surface_va(&[(VA_START, window - (2 << 20))], 4 << 20), None);
        assert!(surface_va(&[(VA_START, window - (2 << 20))], 2 << 20).is_some());
    }
}
