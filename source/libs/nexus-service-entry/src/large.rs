// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the base heap's freed LARGE blocks — the second half of the
//! durable-state free lists (`crate::freelist` holds the small classes): the
//! bounded large-block list TASK-0333 6a planned, pulled forward by TASK-0344.
//!
//! WHY: a block above the largest class went to the bump and never came back.
//! The app-host's frame arena spilled the chat's layout (the boxes of 60+
//! messages needed more than its 256 KiB half, until TASK-0333 6b-2 reserved
//! the arena for the largest frame), so every re-layout carved ~155 KB of
//! large blocks and lost them when the layout was dropped — the 4 MiB heap was
//! full after ~20 keystrokes in the chat and the app died (`alloc-fail`, board
//! 2026-10-22).
//!
//! THE RULE: free blocks are kept in address order in a fixed array. A request
//! takes the FIRST block (lowest address) that holds it and splits off the rest;
//! a freed block merges with the free neighbours it touches. Every block is
//! [`LARGE_ALIGN`]-aligned and a multiple of it ([`large_size`] rounds both the
//! request and the free the same way), so `dealloc`'s layout names exactly the
//! bytes `alloc` handed out. Bounded: at most [`LARGE_SLOTS`] free blocks — a
//! block that finds no slot and no neighbour is dropped and counted (a leak the
//! probe shows, never a corruption); a block overlapping one already free is
//! refused and counted (a double free). Every outcome is a function of the
//! request sequence alone; every operation is O([`LARGE_SLOTS`]).
//!
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the unit tests below (host)
//! ADR: docs/adr/0065-app-host-frame-phase-allocates-from-a-generation-arena.md

/// Alignment and granule of every large block. Requests aligned beyond it go
/// to the bump and never enter the list — on allocation AND on free.
pub(crate) const LARGE_ALIGN: usize = 16;

/// Free blocks the list holds at most.
pub(crate) const LARGE_SLOTS: usize = 32;

/// The bytes a large request occupies — rounded up to [`LARGE_ALIGN`] — or
/// `None` for one the list never serves: aligned beyond it, or too large to be
/// a layout once rounded. ONE function for both paths, so a block returns
/// with the size it was handed out with.
pub(crate) fn large_size(size: usize, align: usize) -> Option<usize> {
    if align > LARGE_ALIGN {
        return None;
    }
    size.checked_add(LARGE_ALIGN - 1)
        .map(|s| s & !(LARGE_ALIGN - 1))
        .filter(|&s| s <= isize::MAX as usize)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Free {
    addr: usize,
    size: usize,
}

/// What became of a freed block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Parked {
    /// On the list (alone or merged with a neighbour).
    Kept,
    /// No slot and no neighbour: the bytes are lost, and counted.
    Dropped,
    /// It overlaps a block already free — a double free; nothing changed.
    Refused,
}

/// The free large blocks, in address order.
pub(crate) struct LargeList {
    free: [Free; LARGE_SLOTS],
    len: usize,
    dropped: usize,
    refused: usize,
}

impl LargeList {
    pub(crate) const fn empty() -> Self {
        Self { free: [Free { addr: 0, size: 0 }; LARGE_SLOTS], len: 0, dropped: 0, refused: 0 }
    }

    /// `size` bytes (a [`large_size`]) from the first free block that holds
    /// them; the rest of that block stays free where it is.
    pub(crate) fn take(&mut self, size: usize) -> Option<usize> {
        let i = self.free[..self.len].iter().position(|b| b.size >= size)?;
        let block = self.free[i];
        if block.size == size {
            self.remove(i);
        } else {
            self.free[i] = Free { addr: block.addr + size, size: block.size - size };
        }
        Some(block.addr)
    }

    /// Returns `size` bytes (a [`large_size`]) at `addr`, merged with the free
    /// neighbours it touches.
    pub(crate) fn park(&mut self, addr: usize, size: usize) -> Parked {
        let i = self.free[..self.len].iter().position(|b| b.addr > addr).unwrap_or(self.len);
        let end = addr.saturating_add(size);
        let prev = (i > 0).then(|| self.free[i - 1]);
        let next = (i < self.len).then(|| self.free[i]);
        let overlaps_prev = prev.is_some_and(|p| p.addr + p.size > addr);
        let overlaps_next = next.is_some_and(|n| end > n.addr);
        if size == 0 || overlaps_prev || overlaps_next {
            self.refused += 1;
            return Parked::Refused;
        }
        let joins_prev = prev.is_some_and(|p| p.addr + p.size == addr);
        let joins_next = next.is_some_and(|n| end == n.addr);
        match (joins_prev, joins_next) {
            (true, true) => {
                self.free[i - 1].size += size + self.free[i].size;
                self.remove(i);
            }
            (true, false) => self.free[i - 1].size += size,
            (false, true) => self.free[i] = Free { addr, size: size + self.free[i].size },
            (false, false) => {
                if self.len == LARGE_SLOTS {
                    self.dropped += 1;
                    return Parked::Dropped;
                }
                self.free.copy_within(i..self.len, i + 1);
                self.free[i] = Free { addr, size };
                self.len += 1;
            }
        }
        Parked::Kept
    }

    fn remove(&mut self, i: usize) {
        self.free.copy_within(i + 1..self.len, i);
        self.len -= 1;
    }

    /// `(blocks, bytes)` free on the list.
    pub(crate) fn free(&self) -> (usize, usize) {
        (self.len, self.free[..self.len].iter().map(|b| b.size).sum())
    }

    /// Frees that lost their bytes (no slot) plus frees refused (a double free).
    pub(crate) fn lost(&self) -> usize {
        self.dropped + self.refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Addresses only: this type never touches memory, it says where bytes go.
    const BASE: usize = 0x10_0000;

    #[test]
    fn requests_round_up_to_the_granule_and_over_aligned_ones_are_the_bumps() {
        assert_eq!(large_size(2049, 8), Some(2064));
        assert_eq!(large_size(0x25000, 8), Some(0x25000));
        assert_eq!(large_size(4096, 16), Some(4096));
        assert_eq!(large_size(4096, 64), None, "over-aligned never enters the list");
        assert_eq!(large_size(isize::MAX as usize - 3, 8), None, "no layout once rounded");
    }

    /// THE property: the frame that spilled gets its bytes back — the same
    /// sizes, re-layout after re-layout, at the same addresses — so a spilling
    /// session's base heap stops growing after its first two frames.
    #[test]
    fn a_freed_block_serves_the_next_request_of_its_size_at_the_same_address() {
        let mut list = LargeList::empty();
        let (boxes, runs) = (0x25000, 0x1400);
        assert_eq!(list.take(boxes), None, "nothing freed yet: the bump carves");
        assert_eq!(list.park(BASE, boxes), Parked::Kept);
        assert_eq!(list.park(BASE + boxes + 0x100, runs), Parked::Kept);
        assert_eq!(list.take(boxes), Some(BASE));
        assert_eq!(list.take(runs), Some(BASE + boxes + 0x100));
        assert_eq!(list.free(), (0, 0));
    }

    /// The board's chat, replayed: every re-layout spills the same three large
    /// blocks (the sizes `alloc-carve` logged while typing) and frees the
    /// previous layout's once it is built. The bump grows for the first two
    /// layouts and never again — without the list it grew ~155 KB a keystroke
    /// until the 4 MiB heap was full.
    #[test]
    fn a_spilling_session_stops_growing_after_two_layouts() {
        let mut list = LargeList::empty();
        let mut cursor = BASE;
        let mut live: Vec<Vec<(usize, usize)>> = Vec::new();
        let mut cursors = Vec::new();
        for _layout in 0..100 {
            let mut blocks = Vec::new();
            for request in [0xa00, 0x1400, 0x25000] {
                let size = large_size(request, 8).expect("a large request");
                let addr = list.take(size).unwrap_or_else(|| {
                    let carved = cursor;
                    cursor += size;
                    carved
                });
                blocks.push((addr, size));
            }
            live.push(blocks);
            if live.len() > 1 {
                for (addr, size) in live.remove(0) {
                    assert_eq!(list.park(addr, size), Parked::Kept);
                }
            }
            cursors.push(cursor);
        }
        assert_eq!(cursors[1], cursors[99], "the bump stopped after two layouts");
        assert_eq!(list.lost(), 0);
    }

    #[test]
    fn a_smaller_request_splits_the_first_block_that_fits_and_leaves_the_rest() {
        let mut list = LargeList::empty();
        assert_eq!(list.park(BASE, 0x4000), Parked::Kept);
        assert_eq!(list.park(BASE + 0x10000, 0x8000), Parked::Kept);
        assert_eq!(list.take(0x6000), Some(BASE + 0x10000), "first fit: the first that HOLDS it");
        assert_eq!(list.take(0x1000), Some(BASE), "lowest address first");
        assert_eq!(list.free(), (2, 0x3000 + 0x2000));
        assert_eq!(list.take(0x2000), Some(BASE + 0x1000), "the first block's split-off rest");
        assert_eq!(list.take(0x2000), Some(BASE + 0x16000), "then the second's");
        assert_eq!(list.free(), (1, 0x1000));
    }

    /// Freed neighbours merge — a block between two free ones joins both — so
    /// a later, larger request still finds room where three smaller ones were.
    #[test]
    fn freed_neighbours_merge_into_one_block() {
        let mut list = LargeList::empty();
        assert_eq!(list.park(BASE, 0x1000), Parked::Kept);
        assert_eq!(list.park(BASE + 0x2000, 0x1000), Parked::Kept);
        assert_eq!(list.free(), (2, 0x2000));
        assert_eq!(list.park(BASE + 0x1000, 0x1000), Parked::Kept);
        assert_eq!(list.free(), (1, 0x3000), "prev + block + next are one");
        assert_eq!(list.take(0x3000), Some(BASE));
    }

    #[test]
    fn test_reject_a_double_free() {
        let mut list = LargeList::empty();
        assert_eq!(list.park(BASE, 0x2000), Parked::Kept);
        assert_eq!(list.park(BASE, 0x2000), Parked::Refused, "the same block twice");
        assert_eq!(list.park(BASE + 0x1000, 0x2000), Parked::Refused, "overlapping its tail");
        assert_eq!(list.park(BASE - 0x1000, 0x2000), Parked::Refused, "overlapping its head");
        assert_eq!(list.free(), (1, 0x2000), "nothing changed");
        assert_eq!(list.lost(), 3);
        assert_eq!(list.park(BASE, 0), Parked::Refused, "an empty block is no block");
    }

    /// Bounded: a full list loses a block that joins nothing — counted — and
    /// still takes one that merges with a neighbour.
    #[test]
    fn test_reject_growing_past_its_slots() {
        let mut list = LargeList::empty();
        for k in 0..LARGE_SLOTS {
            assert_eq!(list.park(BASE + k * 0x2000, 0x1000), Parked::Kept);
        }
        assert_eq!(list.park(BASE + LARGE_SLOTS * 0x2000 + 0x8000, 0x1000), Parked::Dropped);
        assert_eq!(list.lost(), 1);
        assert_eq!(list.park(BASE + 0x1000, 0x1000), Parked::Kept, "a neighbour needs no slot");
        assert_eq!(list.free().0, LARGE_SLOTS - 1, "two blocks merged into one");
    }
}
