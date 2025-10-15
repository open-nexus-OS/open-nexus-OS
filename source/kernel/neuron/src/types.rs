// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal newtypes for safer syscall decoding (debug-friendly, low overhead).

use crate::mm::PAGE_SIZE;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VirtAddr(usize);

impl VirtAddr {
    #[inline]
    pub fn page_aligned(addr: usize) -> Option<Self> {
        if addr % PAGE_SIZE == 0 { Some(Self(addr)) } else { None }
    }
    #[inline]
    pub fn raw(self) -> usize { self.0 }
    #[inline]
    pub fn checked_add(self, v: usize) -> Option<usize> { self.0.checked_add(v) }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PageLen(usize);

impl PageLen {
    #[inline]
    pub fn from_bytes_aligned(bytes: u64) -> Option<Self> {
        if bytes == 0 { return None; }
        let b = bytes as usize;
        if b % PAGE_SIZE == 0 { Some(Self(b)) } else { None }
    }
    #[inline]
    pub fn raw(self) -> usize { self.0 }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SlotIndex(pub usize);

impl SlotIndex {
    #[inline]
    pub fn decode(value: usize) -> Self { Self(value) }
}
