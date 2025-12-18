// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(test, target_arch = "riscv64", target_os = "none"))]
//! CONTEXT: Unit tests for Sv39 page table invariants
//! OWNERS: @kernel-mm-team
//! NOTE: Tests only; verify alignment, flags, W^X, overlap, range, lookup

use super::{MapError, PageFlags, PAGE_SIZE};
use crate::mm::page_table::PageTable;

#[test]
fn rejects_unaligned_addresses() {
    let mut table = PageTable::new();
    assert_eq!(
        table.map(1, PAGE_SIZE, PageFlags::VALID | PageFlags::READ),
        Err(MapError::Unaligned)
    );
    assert_eq!(
        table.map(0, 1, PageFlags::VALID | PageFlags::READ),
        Err(MapError::Unaligned)
    );
}

#[test]
fn rejects_invalid_flags() {
    let mut table = PageTable::new();
    assert_eq!(
        table.map(0, 0, PageFlags::empty()),
        Err(MapError::InvalidFlags)
    );
    assert_eq!(
        table.map(0, 0, PageFlags::VALID),
        Err(MapError::InvalidFlags)
    );
}

#[test]
fn enforces_w_xor_x() {
    let mut table = PageTable::new();
    let flags = PageFlags::VALID | PageFlags::WRITE | PageFlags::EXECUTE;
    assert_eq!(table.map(0, 0, flags), Err(MapError::PermissionDenied));
}

#[test]
fn detects_overlap() {
    let mut table = PageTable::new();
    table
        .map(0, 0, PageFlags::VALID | PageFlags::READ)
        .expect("first mapping");
    assert_eq!(
        table.map(0, PAGE_SIZE, PageFlags::VALID | PageFlags::READ),
        Err(MapError::Overlap)
    );
}

#[test]
fn out_of_range_rejected() {
    let mut table = PageTable::new();
    let va = 1usize << 50; // beyond canonical Sv39 range
    assert_eq!(
        table.map(va, 0, PageFlags::VALID | PageFlags::READ),
        Err(MapError::OutOfRange)
    );
}

#[test]
fn lookup_observes_mapping() {
    let mut table = PageTable::new();
    table
        .map(0, PAGE_SIZE, PageFlags::VALID | PageFlags::READ)
        .expect("map");
    assert_eq!(
        table.lookup(0),
        Some(PAGE_SIZE | (PageFlags::VALID | PageFlags::READ).bits())
    );
    assert_eq!(table.lookup(PAGE_SIZE), None);
}

#[test]
fn root_ppn_reports_base_page() {
    let table = PageTable::new();
    assert_ne!(table.root_ppn(), 0);
}
