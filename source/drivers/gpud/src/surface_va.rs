// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Where an app surface object lies in windowd's GPU context on the board (RFC-0114 §1)
//! — pure, host-tested. From the tail of the context's private GiB (the scratches already open
//! its directory) up to the layout's GiB (TASK-0351 P2, ADR-0093: the windows are bounded by
//! memory, not by this window — a GiB entered costs one directory), one first-fit range per
//! object on a 2 MiB boundary (a page table of its own each), a freed object's range reused.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below incl. `test_reject_a_range_past_the_window`

use alloc::vec::Vec;

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
    fn test_reject_a_range_past_the_window() {
        let window = VA_END - VA_START;
        assert_eq!(surface_va(&[], window + 1), None);
        assert_eq!(surface_va(&[(VA_START, window - (2 << 20))], 4 << 20), None);
        assert!(surface_va(&[(VA_START, window - (2 << 20))], 2 << 20).is_some());
    }
}
