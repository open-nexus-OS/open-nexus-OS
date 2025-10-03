// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]

use super::{MapError, PageFlags, PageTable, PAGE_SIZE};

#[test]
fn rejects_unaligned_addresses() {
    let mut table = PageTable::new();
    assert_eq!(table.map(1, PAGE_SIZE, PageFlags::VALID), Err(MapError::Unaligned));
    assert_eq!(table.map(0, 1, PageFlags::VALID), Err(MapError::Unaligned));
}

#[test]
fn rejects_invalid_flags() {
    let mut table = PageTable::new();
    assert_eq!(table.map(0, 0, PageFlags::empty()), Err(MapError::InvalidFlags));
}

#[test]
fn detects_overlap() {
    let mut table = PageTable::new();
    table.map(0, 0, PageFlags::VALID | PageFlags::READ).unwrap();
    assert_eq!(
        table.map(0, PAGE_SIZE, PageFlags::VALID | PageFlags::READ),
        Err(MapError::Overlap)
    );
}

#[test]
fn out_of_range_rejected() {
    let mut table = PageTable::new();
    assert_eq!(
        table.map(PAGE_SIZE * 1024, PAGE_SIZE * 2, PageFlags::VALID | PageFlags::READ),
        Err(MapError::OutOfRange)
    );
}
