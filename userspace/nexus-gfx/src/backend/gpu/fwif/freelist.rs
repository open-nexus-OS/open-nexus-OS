// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The free list of parameter memory (TASK-0215 5a, RFC-0101 §8d): the open userspace driver's
//! parameters with the open kernel driver's checks, the firmware struct the kernel initialises
//! (`ready_pages`, the stack top, the 16-byte-aligned current address), and the page-number
//! entry the device writes for each of its PM pages. Part of [`super`] (`fwif`).

use core::mem::size_of;

use super::{put32, put64};

/// A free list's parameters as the open userspace driver creates it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreelistArgs {
    /// The list buffer: `max_pages` entries of 4 bytes.
    pub gpu_addr: u64,
    pub initial_pages: u32,
    pub max_pages: u32,
    pub grow_pages: u32,
    pub grow_threshold_percent: u32,
}

/// Why a free list was refused — the open kernel driver's checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreelistError {
    /// initial > max, grow > max, max 0, growth needed but zero, or growth given but full.
    Bounds,
    /// A page count not a multiple of four (the 16-byte alignment of the stack pointer).
    Alignment,
    /// Fewer initial pages than the core's minimum.
    BelowMinimum { min: u32 },
}

/// Entries per 16-byte-aligned group.
const ALIGN_PAGES: u32 = 4;

impl FreelistArgs {
    /// The open kernel driver's checks; `min_pages` is the core's (25 on this one).
    pub fn check(&self, min_pages: u32) -> Result<(), FreelistError> {
        if self.initial_pages > self.max_pages
            || self.grow_pages > self.max_pages
            || self.max_pages == 0
            || (self.initial_pages < self.max_pages && self.grow_pages == 0)
            || (self.initial_pages == self.max_pages && self.grow_pages != 0)
        {
            return Err(FreelistError::Bounds);
        }
        if (self.initial_pages | self.max_pages | self.grow_pages) % ALIGN_PAGES != 0 {
            return Err(FreelistError::Alignment);
        }
        if self.initial_pages < min_pages {
            return Err(FreelistError::BelowMinimum { min: min_pages });
        }
        Ok(())
    }

    /// Pages the firmware may take without asking: the threshold of the initial pages, at
    /// most one growth, rounded down to the alignment.
    pub const fn ready_pages(&self) -> u32 {
        let ready = self.initial_pages * self.grow_threshold_percent / 100;
        let ready = if ready < self.grow_pages { ready } else { self.grow_pages };
        ready & !(ALIGN_PAGES - 1)
    }

    /// The entry index the initial pages start at: the list fills from its end.
    pub const fn first_entry(&self) -> u32 {
        self.max_pages - self.initial_pages
    }

    /// Bytes of the list buffer.
    pub const fn list_bytes(&self) -> usize {
        self.max_pages as usize * 4
    }
}

/// The free list's firmware struct.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Freelist {
    pub freelist_dev_addr: u64,
    pub current_dev_addr: u64,
    pub current_stack_top: u32,
    pub max_pages: u32,
    pub grow_pages: u32,
    pub current_pages: u32,
    pub allocated_page_count: u32,
    pub allocated_mmu_page_count: u32,
    pub freelist_id: u32,
    pub grow_pending: u32,
    pub ready_pages: u32,
    pub freelist_flags: u32,
    pub pm_global_pb: u32,
    pub padding: u32,
}

impl Freelist {
    pub const SIZE: usize = 64;

    /// The open kernel driver's `free_list_fw_init`; `id` is the device's number for the list.
    pub fn new(args: &FreelistArgs, id: u32) -> Self {
        let ready = args.ready_pages();
        let current = args.initial_pages - ready;
        Self {
            freelist_dev_addr: args.gpu_addr,
            current_dev_addr: (args.gpu_addr + u64::from(args.max_pages - current) * 4) & !15,
            current_stack_top: current.wrapping_sub(1),
            max_pages: args.max_pages,
            grow_pages: args.grow_pages,
            current_pages: current,
            allocated_page_count: 0,
            allocated_mmu_page_count: 0,
            freelist_id: id,
            grow_pending: 0,
            ready_pages: ready,
            freelist_flags: 0,
            pm_global_pb: 0,
            padding: 0,
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put64(&mut b, 0, self.freelist_dev_addr);
        put64(&mut b, 8, self.current_dev_addr);
        for (i, v) in [
            self.current_stack_top,
            self.max_pages,
            self.grow_pages,
            self.current_pages,
            self.allocated_page_count,
            self.allocated_mmu_page_count,
            self.freelist_id,
            self.grow_pending,
            self.ready_pages,
            self.freelist_flags,
            self.pm_global_pb,
        ]
        .iter()
        .enumerate()
        {
            put32(&mut b, 16 + i * 4, *v);
        }
        b
    }
}

/// The page-number entry of one PM page: its physical address in 4 KiB units.
pub const fn page_entry(phys: u64) -> u32 {
    (phys >> 12) as u32
}

const _: () = assert!(size_of::<Freelist>() == Freelist::SIZE);
