// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: one memory bank's buddy state — a bitmap per order (bit set = a
//! block is free at exactly that order) with a one-bit-per-word summary above
//! each, so "the lowest free block of this order" is a scan of the summary,
//! one word, and a `trailing_zeros`. Indices count frames from `origin`, the
//! bank base rounded DOWN to a superpage, so every order-`MAX_ORDER` block is
//! superpage-aligned in physical memory whatever the bank base is; the frames
//! between `origin` and the first allocatable one are never free. The frames
//! themselves are never touched: the metadata is a boxed slice, so the same
//! code runs over a 4 GiB board tree on the host with no memory behind it.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `super::tests` (goldens, merge round trip, rejects, proptest)
//! INVARIANTS: a summary bit is set iff its word is non-zero; a set bit at
//!   order `o` covers frames that are free at NO other order; a block that
//!   overlaps a hole is never free.

use alloc::boxed::Box;
use alloc::vec;

use super::{
    align_down, align_up, Block, FrameError, Range, FRAME_SHIFT, FRAME_SIZE, MAX_HOLES, MAX_ORDER,
};

const ORDERS: usize = MAX_ORDER as usize + 1;
/// The superpage the `MAX_ORDER` block is (2 MiB with 4 KiB frames).
const SUPER_ALIGN: u64 = FRAME_SIZE << MAX_ORDER;

/// Where one order's bitmap and summary live inside `Bank::bits`.
#[derive(Clone, Copy, Default)]
struct Level {
    /// Block slots at this order (a partial tail block has a slot that is never set).
    blocks: usize,
    /// First word of the bitmap.
    bits: usize,
    /// First word of the summary (one bit per bitmap word).
    sum: usize,
    sum_words: usize,
}

/// One bank of physical frames under buddy management.
pub struct Bank {
    origin: u64,
    base: u64,
    end: u64,
    /// Index space: frames from `origin` to `end`.
    frames: usize,
    total: usize,
    free: usize,
    levels: [Level; ORDERS],
    bits: Box<[u64]>,
    /// Holes clipped to this bank, frame-expanded: a `free` of a block that
    /// touches one is refused (nothing in a hole ever had an owner).
    holes: [Range; MAX_HOLES],
    hole_count: usize,
}

impl Bank {
    /// Metadata for the frame-aligned inside of `range`; nothing is free yet.
    pub fn new(range: Range) -> Result<Self, FrameError> {
        let base = align_up(range.base, FRAME_SIZE);
        let end = align_down(range.end().ok_or(FrameError::TooLarge)?, FRAME_SIZE);
        let origin = align_down(base, SUPER_ALIGN);
        let frames = if end > base {
            usize::try_from((end - origin) >> FRAME_SHIFT).map_err(|_| FrameError::TooLarge)?
        } else {
            0
        };
        let mut levels = [Level::default(); ORDERS];
        let mut words = 0usize;
        for (order, level) in levels.iter_mut().enumerate() {
            let blocks = frames.div_ceil(1usize << order);
            let bit_words = blocks.div_ceil(64);
            let sum_words = bit_words.div_ceil(64);
            *level = Level { blocks, bits: words, sum: words + bit_words, sum_words };
            words += bit_words + sum_words;
        }
        Ok(Self {
            origin,
            base,
            end: end.max(base),
            frames,
            total: 0,
            free: 0,
            levels,
            bits: vec![0u64; words].into_boxed_slice(),
            holes: [Range { base: 0, size: 0 }; MAX_HOLES],
            hole_count: 0,
        })
    }

    /// First allocatable byte (frame-aligned up from the bank's `reg`).
    pub fn base(&self) -> u64 {
        self.base
    }

    /// One past the last allocatable byte.
    pub fn end(&self) -> u64 {
        self.end
    }

    /// Frames that were ever handed to this bank as free.
    pub fn total_frames(&self) -> usize {
        self.total
    }

    /// Frames free right now.
    pub fn free_frames(&self) -> usize {
        self.free
    }

    /// Metadata words this bank's bitmaps occupy (for the boot report).
    pub fn metadata_words(&self) -> usize {
        self.bits.len()
    }

    /// `pa` lies inside this bank's allocatable extent.
    pub fn contains(&self, pa: u64) -> bool {
        pa >= self.base && pa < self.end
    }

    /// Remember a hole (already clipped and frame-expanded by the caller).
    pub fn record_hole(&mut self, hole: Range) -> Result<(), FrameError> {
        if self.hole_count == MAX_HOLES {
            return Err(FrameError::TooManyHoles);
        }
        self.holes[self.hole_count] = hole;
        self.hole_count += 1;
        Ok(())
    }

    fn touches_hole(&self, block: Block) -> bool {
        let (start, end) = (block.base, block.base + block.size());
        self.holes[..self.hole_count].iter().any(|h| start < h.base + h.size && h.base < end)
    }

    /// Hand `[start, end)` (physical) to the bank as free blocks: the largest
    /// aligned block first, so a 2 MiB-aligned run becomes one order-9 block
    /// and the tails become the smallest blocks that fit. Init only.
    pub fn add_region(&mut self, start: u64, end: u64) {
        let start = align_up(start.max(self.base), FRAME_SIZE);
        let end = align_down(end.min(self.end), FRAME_SIZE);
        if end <= start {
            return;
        }
        // Inside `[origin, end)` by construction, so the casts cannot truncate
        // more than `frames` did.
        let mut cur = ((start - self.origin) >> FRAME_SHIFT) as usize;
        let stop = ((end - self.origin) >> FRAME_SHIFT) as usize;
        while cur < stop {
            let align = if cur == 0 { u32::from(MAX_ORDER) } else { cur.trailing_zeros() };
            let span = (stop - cur).ilog2();
            let order = align.min(span).min(u32::from(MAX_ORDER)) as u8;
            self.insert(order, cur >> order);
            self.total += 1usize << order;
            self.free += 1usize << order;
            cur += 1usize << order;
        }
    }

    /// The lowest free block of `order` or larger, split down to `order`.
    pub fn alloc(&mut self, order: u8) -> Option<Block> {
        for o in order..=MAX_ORDER {
            let Some(idx) = self.first_free(o) else { continue };
            self.clear(o, idx);
            let mut i = idx;
            for k in (order..o).rev() {
                i <<= 1;
                self.set(k, i + 1);
            }
            self.free -= 1usize << order;
            let base = self.origin + ((i as u64) << (u32::from(order) + FRAME_SHIFT));
            return Some(Block { base, order });
        }
        None
    }

    /// Return `block`; buddies merge upward. Refuses blocks this bank never
    /// handed out: outside its extent, misaligned for their order, touching a
    /// hole, or already free (a double free).
    pub fn free(&mut self, block: Block) -> Result<(), FrameError> {
        if block.order > MAX_ORDER {
            return Err(FrameError::BadOrder);
        }
        if !self.contains(block.base) || block.base % block.size() != 0 {
            return Err(FrameError::NotOwned);
        }
        let idx = ((block.base - self.origin) >> (u32::from(block.order) + FRAME_SHIFT)) as usize;
        if !self.fits(block.order, idx) || self.touches_hole(block) {
            return Err(FrameError::NotOwned);
        }
        let (mut o, mut i) = (block.order, idx);
        loop {
            if self.is_free(o, i) {
                return Err(FrameError::NotAllocated);
            }
            if o == MAX_ORDER {
                break;
            }
            o += 1;
            i >>= 1;
        }
        #[cfg(debug_assertions)]
        debug_assert!(!self.descendant_free(block.order, idx), "free of a partially free block");
        self.insert(block.order, idx);
        self.free += 1usize << block.order;
        Ok(())
    }

    /// Insert a block, merging with its buddy while the buddy is free.
    fn insert(&mut self, mut order: u8, mut idx: usize) {
        while order < MAX_ORDER {
            let buddy = idx ^ 1;
            if !(self.fits(order, buddy) && self.is_free(order, buddy)) {
                break;
            }
            self.clear(order, buddy);
            idx >>= 1;
            order += 1;
        }
        self.set(order, idx);
    }

    /// The block's last frame lies inside the index space.
    fn fits(&self, order: u8, idx: usize) -> bool {
        idx < self.levels[order as usize].blocks && (idx + 1) << order <= self.frames
    }

    fn is_free(&self, order: u8, idx: usize) -> bool {
        let level = self.levels[order as usize];
        idx < level.blocks && self.bits[level.bits + idx / 64] & (1u64 << (idx % 64)) != 0
    }

    #[cfg(debug_assertions)]
    fn descendant_free(&self, order: u8, idx: usize) -> bool {
        (0..order).any(|o| {
            let shift = order - o;
            (idx << shift..(idx + 1) << shift).any(|i| self.is_free(o, i))
        })
    }

    fn set(&mut self, order: u8, idx: usize) {
        let level = self.levels[order as usize];
        let word = idx / 64;
        self.bits[level.bits + word] |= 1u64 << (idx % 64);
        self.bits[level.sum + word / 64] |= 1u64 << (word % 64);
    }

    fn clear(&mut self, order: u8, idx: usize) {
        let level = self.levels[order as usize];
        let word = idx / 64;
        self.bits[level.bits + word] &= !(1u64 << (idx % 64));
        if self.bits[level.bits + word] == 0 {
            self.bits[level.sum + word / 64] &= !(1u64 << (word % 64));
        }
    }

    /// Lowest free block index at `order`: summary scan, one word, one `ctz`.
    fn first_free(&self, order: u8) -> Option<usize> {
        let level = self.levels[order as usize];
        let sum = &self.bits[level.sum..level.sum + level.sum_words];
        let (si, sw) = sum.iter().enumerate().find(|(_, w)| **w != 0)?;
        let word = si * 64 + sw.trailing_zeros() as usize;
        Some(word * 64 + self.bits[level.bits + word].trailing_zeros() as usize)
    }
}
