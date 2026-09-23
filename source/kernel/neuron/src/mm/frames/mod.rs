// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the page-frame allocator (TASK-0286 M1, RFC-0098 C4): physical
//! memory is what the tree's `/memory` banks say minus what the tree reserves
//! and what the kernel excludes (its image, the tree), under a buddy per bank
//! (`bank.rs`). Allocation is first-fit in bank order — the lowest free block
//! of the smallest sufficient order — so the frames a given tree yields for a
//! given call sequence are the same on every boot (the smp1 lane leans on
//! that). Blocks are physically contiguous and superpage-aligned at
//! `MAX_ORDER` by construction, which is what `contiguous-DMA` (P4) and the
//! 2 MiB promotion in `vm_ops` need. NOT target-gated (pure index logic over a
//! boxed bitmap) so the goldens below run on host — same shape as
//! `image_allocs`/`va_space`; the kernel wires it in P2 (direct map).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (host-proven; kernel consumers arrive with P2/P3)
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests.rs` — both golden trees, determinism, merge round
//!   trip, rejects (double free, foreign block, hole), exhaustion, proptest
//! INVARIANTS: every frame has ONE owner (a block or nobody); a hole or a
//!   frame outside every bank is never handed out and never accepted back;
//!   exhaustion is an error AND a counter, never a silent `None`; the cost of
//!   an allocation is bounded by orders × summary words (≤ 10 × 128 for a
//!   2 GiB bank), a free by orders.

extern crate alloc;

use alloc::vec::Vec;

mod bank;
#[cfg(test)]
mod tests;

pub use bank::Bank;

/// log2 of the frame size.
pub const FRAME_SHIFT: u32 = 12;
/// One frame: the Sv39 base page.
pub const FRAME_SIZE: u64 = 1 << FRAME_SHIFT;
/// The largest block order: 2^9 frames = one 2 MiB superpage.
pub const MAX_ORDER: u8 = 9;
/// Banks a tree may carry (the board has two; a NUMA box is out of scope).
pub const MAX_BANKS: usize = 4;
/// Reserved + excluded ranges an init may carry, in total.
pub const MAX_HOLES: usize = 32;

/// A physical byte range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub base: u64,
    pub size: u64,
}

impl Range {
    /// Exclusive end, `None` when it does not fit in 64 bits.
    pub fn end(self) -> Option<u64> {
        self.base.checked_add(self.size)
    }
}

/// A block of `1 << order` physically contiguous frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub base: u64,
    pub order: u8,
}

impl Block {
    /// Bytes covered.
    pub fn size(self) -> u64 {
        FRAME_SIZE << self.order
    }

    /// Frames covered.
    pub fn frames(self) -> usize {
        1usize << self.order
    }

    /// Exclusive end.
    pub fn end(self) -> u64 {
        self.base + self.size()
    }
}

/// Why an allocator call was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// No free block of `order` in any bank; `free` is the frames still free
    /// (in smaller blocks, or above a `alloc_below` limit).
    Exhausted { order: u8, free: usize },
    /// `order > MAX_ORDER`.
    BadOrder,
    /// The block lies outside every bank, is misaligned for its order, or
    /// touches a hole — nothing this allocator ever handed out.
    NotOwned,
    /// The block (or a block containing it) is already free: a double free.
    NotAllocated,
    /// More `/memory` banks than `MAX_BANKS`.
    TooManyBanks,
    /// More reserved + excluded ranges than `MAX_HOLES`.
    TooManyHoles,
    /// A range whose end or frame count does not fit the machine word.
    TooLarge,
}

/// Counters for the boot report and the telemetry query (P5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub banks: usize,
    /// Frames ever handed to the buddies (bank extents minus holes).
    pub total: usize,
    pub free: usize,
    /// Frames the tree reserved, clipped to the banks.
    pub reserved: usize,
    /// Frames the kernel excluded, clipped to the banks.
    pub excluded: usize,
    pub allocs: u64,
    pub frees: u64,
    pub exhausted: u64,
}

/// The allocator over every bank of the tree.
pub struct FrameAllocator {
    banks: Vec<Bank>,
    reserved: usize,
    excluded: usize,
    allocs: u64,
    frees: u64,
    exhausted: u64,
}

impl FrameAllocator {
    /// Build over `banks`, carving out `reserved` (the tree's) and `excluded`
    /// (the kernel's) ranges. A hole that covers part of a frame poisons the
    /// whole frame.
    pub fn init(
        banks: &[Range],
        reserved: &[Range],
        excluded: &[Range],
    ) -> Result<Self, FrameError> {
        if banks.len() > MAX_BANKS {
            return Err(FrameError::TooManyBanks);
        }
        if reserved.len() + excluded.len() > MAX_HOLES {
            return Err(FrameError::TooManyHoles);
        }
        let mut out =
            Self { banks: Vec::new(), reserved: 0, excluded: 0, allocs: 0, frees: 0, exhausted: 0 };
        for range in banks {
            let mut bank = Bank::new(*range)?;
            let mut holes = [Range { base: 0, size: 0 }; MAX_HOLES];
            let mut count = 0usize;
            for (list, counter) in [(reserved, &mut out.reserved), (excluded, &mut out.excluded)] {
                for hole in list {
                    let Some(clipped) = clip_to_frames(*hole, bank.base(), bank.end()) else {
                        continue;
                    };
                    *counter += (clipped.size >> FRAME_SHIFT) as usize;
                    bank.record_hole(clipped)?;
                    holes[count] = clipped;
                    count += 1;
                }
            }
            sort_by_base(&mut holes[..count]);
            let mut cursor = bank.base();
            for hole in &holes[..count] {
                if hole.base > cursor {
                    bank.add_region(cursor, hole.base);
                }
                cursor = cursor.max(hole.base + hole.size);
            }
            bank.add_region(cursor, bank.end());
            out.banks.push(bank);
        }
        Ok(out)
    }

    /// `init` over the tree's `/memory` banks and reserved ranges (RFC-0098 C4).
    pub fn init_from_fdt(fdt: &nexus_fdt::Fdt<'_>, excluded: &[Range]) -> Result<Self, FrameError> {
        let mut banks = [Range { base: 0, size: 0 }; MAX_BANKS];
        let mut nbanks = 0usize;
        for bank in fdt.memory_banks() {
            if nbanks == MAX_BANKS {
                return Err(FrameError::TooManyBanks);
            }
            banks[nbanks] = Range { base: bank.base, size: bank.size };
            nbanks += 1;
        }
        let mut reserved = [Range { base: 0, size: 0 }; MAX_HOLES];
        let mut nreserved = 0usize;
        for range in fdt.reserved_ranges() {
            if nreserved == MAX_HOLES {
                return Err(FrameError::TooManyHoles);
            }
            reserved[nreserved] = Range { base: range.base, size: range.size };
            nreserved += 1;
        }
        Self::init(&banks[..nbanks], &reserved[..nreserved], excluded)
    }

    /// The lowest free block of `order` across the banks in tree order.
    pub fn alloc(&mut self, order: u8) -> Result<Block, FrameError> {
        self.alloc_below(order, u64::MAX)
    }

    /// Like `alloc`, but the block must end at or below `limit` (a DMA master
    /// with a 32-bit address window asks for `1 << 32`). First-fit makes the
    /// answer per bank exact: if the lowest block is above the limit, none is
    /// below it.
    pub fn alloc_below(&mut self, order: u8, limit: u64) -> Result<Block, FrameError> {
        if order > MAX_ORDER {
            return Err(FrameError::BadOrder);
        }
        for bank in self.banks.iter_mut().filter(|b| b.base() < limit) {
            let Some(block) = bank.alloc(order) else { continue };
            if block.end() <= limit {
                self.allocs += 1;
                return Ok(block);
            }
            // Never handed out, so this cannot fail.
            let _ = bank.free(block);
        }
        self.exhausted += 1;
        Err(FrameError::Exhausted { order, free: self.free_frames() })
    }

    /// Return a block to the bank it came from.
    pub fn free(&mut self, block: Block) -> Result<(), FrameError> {
        if block.order > MAX_ORDER {
            return Err(FrameError::BadOrder);
        }
        let bank =
            self.banks.iter_mut().find(|b| b.contains(block.base)).ok_or(FrameError::NotOwned)?;
        bank.free(block)?;
        self.frees += 1;
        Ok(())
    }

    /// Frames free across every bank.
    pub fn free_frames(&self) -> usize {
        self.banks.iter().map(Bank::free_frames).sum()
    }

    /// The banks, in tree order.
    pub fn banks(&self) -> &[Bank] {
        &self.banks
    }

    /// The counters.
    pub fn stats(&self) -> Stats {
        Stats {
            banks: self.banks.len(),
            total: self.banks.iter().map(Bank::total_frames).sum(),
            free: self.free_frames(),
            reserved: self.reserved,
            excluded: self.excluded,
            allocs: self.allocs,
            frees: self.frees,
            exhausted: self.exhausted,
        }
    }
}

/// `hole` clipped to `[base, end)` and widened to whole frames; `None` when
/// they do not meet.
fn clip_to_frames(hole: Range, base: u64, end: u64) -> Option<Range> {
    let hole_end = hole.end()?;
    if hole.size == 0 || hole.base >= end || hole_end <= base {
        return None;
    }
    let start = align_down(hole.base.max(base), FRAME_SIZE);
    let stop = align_up(hole_end.min(end), FRAME_SIZE);
    (stop > start).then(|| Range { base: start, size: stop - start })
}

/// Insertion sort: the hole list is bounded by `MAX_HOLES`.
fn sort_by_base(holes: &mut [Range]) {
    for i in 1..holes.len() {
        let mut j = i;
        while j > 0 && holes[j - 1].base > holes[j].base {
            holes.swap(j - 1, j);
            j -= 1;
        }
    }
}

pub(crate) fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

/// Saturates at the top of the address space rather than wrapping to 0.
pub(crate) fn align_up(value: u64, align: u64) -> u64 {
    value.checked_add(align - 1).map_or(u64::MAX & !(align - 1), |v| v & !(align - 1))
}
