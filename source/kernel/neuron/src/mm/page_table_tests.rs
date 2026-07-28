// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for [`super::page_table`] — split out of the module
//! (structure-gate). HONESTY NOTE: `mod mm` itself is target-gated
//! (`lib.rs`), so like `mm/tests.rs` these compile ONLY in a RISC-V test
//! build, which no lane runs — they are documentation-grade until the mm
//! host-test story lands (RFC-0085 puts the live host oracle in the un-gated
//! `va_space` pure module instead).

use super::page_table::*;

use super::*;

#[test]
fn map_2m_translates_entire_huge_page() {
    let mut table = PageTable::new();
    table
        .map_2m(HUGE_PAGE_SIZE_2M, HUGE_PAGE_SIZE_2M * 2, PageFlags::VALID | PageFlags::READ)
        .expect("2m mapping");

    assert_eq!(table.translate(HUGE_PAGE_SIZE_2M), Some(HUGE_PAGE_SIZE_2M * 2));
    assert_eq!(
        table.translate(HUGE_PAGE_SIZE_2M + PAGE_SIZE),
        Some(HUGE_PAGE_SIZE_2M * 2 + PAGE_SIZE)
    );
    assert_eq!(
        table.leaf_flags(HUGE_PAGE_SIZE_2M).expect("leaf flags"),
        PageFlags::VALID | PageFlags::READ | PageFlags::ACCESSED
    );
}

#[test]
fn map_2m_rejects_unaligned_or_wx_mappings() {
    let mut table = PageTable::new();
    assert_eq!(
        table.map_2m(PAGE_SIZE, 0, PageFlags::VALID | PageFlags::READ),
        Err(MapError::Unaligned)
    );
    assert_eq!(
        table.map_2m(0, 0, PageFlags::VALID | PageFlags::WRITE | PageFlags::EXECUTE),
        Err(MapError::PermissionDenied)
    );
}

#[test]
fn allocation_stats_track_owned_page_lifetime() {
    let before = PageTable::allocation_stats();
    {
        let mut table = PageTable::new();
        table.map(0, 0, PageFlags::VALID | PageFlags::READ).expect("map");
        let during = PageTable::allocation_stats();
        assert!(during.heap_live >= before.heap_live + 1);
        assert!(during.heap_total >= before.heap_total + 1);
        assert!(during.heap_peak >= during.heap_live);
        assert!(table.allocated_pages() >= 1);
    }
    let after = PageTable::allocation_stats();
    assert_eq!(after.heap_live, before.heap_live);
    assert!(after.heap_total >= before.heap_total + 1);
}
