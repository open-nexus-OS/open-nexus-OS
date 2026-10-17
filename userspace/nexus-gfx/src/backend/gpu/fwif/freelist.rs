// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The free list of parameter memory (TASK-0215 5a, RFC-0101 §8d): the open userspace driver's
//! parameters with the open kernel driver's checks, the firmware struct the kernel initialises
//! (`ready_pages`, the stack top, the 16-byte-aligned current address), and the page-number
//! entry the device writes for each of its PM pages. Part of [`super`] (`fwif`).

use core::mem::{offset_of, size_of};

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
    /// The words a reconstruction after the firmware's hardware recovery rewrites (the open
    /// kernel driver's `pvr_free_list_reconstruct`): the stack from the list's pages, the
    /// allocation counts back to zero.
    pub const CURRENT_DEV_ADDR_OFFSET: usize = 8;
    pub const CURRENT_STACK_TOP_OFFSET: usize = 16;
    pub const CURRENT_PAGES_OFFSET: usize = 28;
    pub const ALLOCATED_PAGE_COUNT_OFFSET: usize = 32;
    pub const ALLOCATED_MMU_PAGE_COUNT_OFFSET: usize = 36;
    pub const READY_PAGES_OFFSET: usize = 48;

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

    /// What a reconstruction writes into the list's struct after the firmware's hardware
    /// recovery, as `(offset, word)`: the list as made — every page it was given back on its
    /// stack (the device never grows a list, so its own count is the list's: no word the
    /// firmware wrote is read), the ready pages reserved again, none allocated. The entries are
    /// written again beside it (each page's number, as at creation).
    pub fn rebuilt(args: &FreelistArgs) -> [(usize, u32); 7] {
        let made = Self::new(args, 0);
        [
            (Self::CURRENT_DEV_ADDR_OFFSET, made.current_dev_addr as u32),
            (Self::CURRENT_DEV_ADDR_OFFSET + 4, (made.current_dev_addr >> 32) as u32),
            (Self::CURRENT_STACK_TOP_OFFSET, made.current_stack_top),
            (Self::CURRENT_PAGES_OFFSET, made.current_pages),
            (Self::ALLOCATED_PAGE_COUNT_OFFSET, 0),
            (Self::ALLOCATED_MMU_PAGE_COUNT_OFFSET, 0),
            (Self::READY_PAGES_OFFSET, made.ready_pages),
        ]
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
const _: () =
    assert!(offset_of!(Freelist, current_stack_top) == Freelist::CURRENT_STACK_TOP_OFFSET);
const _: () = assert!(offset_of!(Freelist, current_pages) == Freelist::CURRENT_PAGES_OFFSET);
const _: () =
    assert!(offset_of!(Freelist, allocated_page_count) == Freelist::ALLOCATED_PAGE_COUNT_OFFSET);
const _: () = assert!(
    offset_of!(Freelist, allocated_mmu_page_count) == Freelist::ALLOCATED_MMU_PAGE_COUNT_OFFSET
);
const _: () = assert!(offset_of!(Freelist, current_dev_addr) == Freelist::CURRENT_DEV_ADDR_OFFSET);
const _: () = assert!(offset_of!(Freelist, ready_pages) == Freelist::READY_PAGES_OFFSET);
