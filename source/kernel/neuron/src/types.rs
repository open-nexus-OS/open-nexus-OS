// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal newtypes for safer syscall decoding (debug-friendly, low overhead).

use crate::mm::{page_table::is_canonical_sv39, PAGE_SIZE};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VirtAddr(usize);

impl VirtAddr {
    #[inline]
    pub fn new(addr: usize) -> Option<Self> {
        if is_canonical_sv39(addr) {
            Some(Self(addr))
        } else {
            None
        }
    }

    #[inline]
    pub fn page_aligned(addr: usize) -> Option<Self> {
        Self::new(addr).and_then(|va| if va.0 % PAGE_SIZE == 0 { Some(va) } else { None })
    }

    #[inline]
    pub fn instr_aligned(addr: usize) -> Option<Self> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        const INSTR_ALIGN: usize = 2; // allow compressed 16-bit alignment on RISC-V
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        const INSTR_ALIGN: usize = core::mem::size_of::<u32>();
        Self::new(addr).and_then(|va| if va.0 % INSTR_ALIGN == 0 { Some(va) } else { None })
    }
    #[inline]
    pub fn raw(self) -> usize {
        self.0
    }
    #[inline]
    pub fn checked_add(self, v: usize) -> Option<usize> {
        self.0.checked_add(v)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PageLen(usize);

impl PageLen {
    #[inline]
    pub fn from_bytes_aligned(bytes: u64) -> Option<Self> {
        if bytes == 0 {
            return None;
        }
        let b = bytes as usize;
        if b % PAGE_SIZE == 0 {
            Some(Self(b))
        } else {
            None
        }
    }
    #[inline]
    pub fn raw(self) -> usize {
        self.0
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SlotIndex(pub usize);

impl SlotIndex {
    #[inline]
    pub fn decode(value: usize) -> Self {
        Self(value)
    }
}
