// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the page table of ONE lazy object (ADR-0077, RFC-0100 §2 and §5) — one word per 4 KiB
//! page: absent (never touched, or purged, or dropped as zeroes: the next touch commits a zeroed
//! frame), a frame, or a RECORD in the object's compressed store (TASK-0333 6f-1, ADR-0087: the
//! word tagged, so no path can mistake a squeezed page for an untouched one). A page is
//! committed once; a squeeze turns a frame into a record and hands the frame back to the caller
//! (who returns it to the pool once no TLB can reach it); an inflate turns the record back into
//! a frame in two steps — the record copied out, decoded by the caller outside the table lock,
//! then entered — so the decode never runs under the lock. A fourth state came with the swap
//! stage (6f-2, ADR-0088): SWAPPED — the record left for the pager's disk in two phases (an export
//! keeps the page a record, the pager's commit makes it swapped; a touch between cancels the
//! export), the slot and the seal's counter in the swap map (`lazy_swap.rs`). Split out of
//! `lazy.rs` (the rules) with the store beside it (`lazy_store.rs`).
//!
//! NOT target-gated (a child of `lazy`).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6b-1; the record state 6f-1)
//! API_STABILITY: Internal
//! TEST_COVERAGE: `lazy_pages_tests.rs` — a page committed once, counts, bounds, the purge; a
//!   page squeezed and brought back byte-exact, a zero page dropped, the store gone with its last
//!   record; a page exported, swapped and back (`test_reject_*`: a commit over a record, a
//!   squeeze of a page that holds no frame, an inflate of a page whose word moved on, a swap
//!   commit after a touch or naming another export, a swap step out of order)
//! INVARIANTS: a page has at most one frame or one record, never both; a frame number fits 31
//!   bits; `committed` counts the frames, `squeezed` the records.

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::store::{PageMemory, Released, Spare, Store, StoreError};
use super::swap::{SwapEntry, SwapError, SwapPages};
use super::PAGE;

/// A page's word: never touched (or purged, or dropped as zeroes).
const NONE: u32 = u32::MAX;
/// The tag of a word that names a record: the rest is the record's first slot (ADR-0087; a slot
/// fits 30 bits, `store::MAX_SLOTS`).
const RECORD: u32 = 1 << 31;
/// The word of a swapped page (ADR-0088): its slot and counter are in the object's swap map.
const SWAPPED: u32 = 0xC000_0000;

/// What one page of a lazy object holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageState {
    /// No frame, no record: the next touch commits a zeroed frame.
    Absent,
    /// The page's frame (physical address).
    Frame(u64),
    /// The page lives as the record starting at this slot of the object's store.
    Record(u32),
    /// The page's sealed record is in the pager's slot (`swap_entry`): a touch parks until it
    /// is back (ADR-0088).
    Swapped,
}

/// Why a commit was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitError {
    /// The page already has this frame: the caller frees the one it brought and uses it.
    Committed(u64),
    /// The page lives as a record: it is inflated, not committed afresh.
    Squeezed,
    /// The page is swapped: it comes back through the pager, not committed afresh.
    Swapped,
    /// The page lies past the object's end, or the frame does not fit the table's word.
    OutOfRange,
}

/// Why a swap step was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapStateError {
    /// The page lies past the object's end, or the frame does not fit the table's word.
    OutOfRange,
    /// An export or a commit needs a record; this page holds none.
    NotARecord,
    /// A swap-in needs a swapped page; this one is not.
    NotSwapped,
    /// The swap map refused (exported already, not exported, swapped already).
    Swap(SwapError),
    /// The store refused to release the record.
    Store(StoreError),
}

/// Why a squeeze or an inflate step was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqueezeError {
    /// The page lies past the object's end, or the frame does not fit the table's word.
    OutOfRange,
    /// The page holds no frame (a squeeze takes only a committed page).
    NotCommitted,
    /// The page holds no record (an inflate brings back only a squeezed page).
    NotSqueezed,
    /// The page's word moved on between the inflate's two steps.
    Changed,
    /// The store refused (no frame for a new store page, a record it does not hold).
    Store(StoreError),
}

/// The pages of a lazy object: one word per 4 KiB page, and the store of its squeezed ones.
#[derive(Debug)]
pub struct LazyPages {
    words: Vec<u32>,
    committed: usize,
    squeezed: usize,
    /// The records of the squeezed pages, made at the first squeeze, gone with the last record.
    store: Option<Box<Store>>,
    /// The exported and swapped pages (ADR-0088): their counters and the pager's slots.
    swap: SwapPages,
}

fn decode(word: u32) -> PageState {
    if word == NONE {
        PageState::Absent
    } else if word == SWAPPED {
        PageState::Swapped
    } else if word & RECORD != 0 {
        PageState::Record(word & !RECORD)
    } else {
        PageState::Frame(u64::from(word) * PAGE as u64)
    }
}

/// `pa`'s word, when it is a page-aligned frame whose number fits 31 bits.
fn frame_word(pa: u64) -> Option<u32> {
    let pfn = u32::try_from(pa / PAGE as u64).ok()?;
    (pfn < RECORD && pa % PAGE as u64 == 0).then_some(pfn)
}

impl LazyPages {
    /// `pages` pages, none committed — `None` when the kernel heap cannot hold their words above
    /// its reserve (ADR-0091): the caller sized the object, so the caller hears `ENOMEM`; the
    /// kernel never panics over it.
    #[must_use]
    pub fn try_new(pages: usize) -> Option<Self> {
        let bytes = pages.checked_mul(core::mem::size_of::<u32>())?;
        if !crate::kheap::admits(bytes) {
            return None;
        }
        let mut words = Vec::new();
        words.try_reserve_exact(pages).ok()?;
        words.resize(pages, NONE);
        Some(Self { words, committed: 0, squeezed: 0, store: None, swap: SwapPages::new() })
    }

    /// `pages` pages, none committed (host tests).
    #[cfg(test)]
    #[must_use]
    pub fn new(pages: usize) -> Self {
        Self::try_new(pages).expect("test object pages")
    }

    /// Pages the object spans.
    #[must_use]
    pub fn pages(&self) -> usize {
        self.words.len()
    }

    /// Pages committed (holding a frame).
    #[must_use]
    pub fn committed(&self) -> usize {
        self.committed
    }

    /// Pages held as records.
    #[must_use]
    pub fn squeezed(&self) -> usize {
        self.squeezed
    }

    /// What page `index` holds, `None` past the end.
    #[must_use]
    pub fn state(&self, index: usize) -> Option<PageState> {
        self.words.get(index).copied().map(decode)
    }

    /// The physical address of page `index`'s frame, when committed.
    #[must_use]
    pub fn frame(&self, index: usize) -> Option<u64> {
        match self.state(index)? {
            PageState::Frame(pa) => Some(pa),
            _ => None,
        }
    }

    /// Record `pa` as page `index`'s frame. A page is committed once: a second commit is refused
    /// with the frame the page already has, a commit over a record with `Squeezed`.
    pub fn commit(&mut self, index: usize, pa: u64) -> Result<(), CommitError> {
        let word = frame_word(pa).ok_or(CommitError::OutOfRange)?;
        let entry = self.words.get_mut(index).ok_or(CommitError::OutOfRange)?;
        match decode(*entry) {
            PageState::Absent => {}
            PageState::Frame(existing) => return Err(CommitError::Committed(existing)),
            PageState::Record(_) => return Err(CommitError::Squeezed),
            PageState::Swapped => return Err(CommitError::Swapped),
        }
        *entry = word;
        self.committed += 1;
        Ok(())
    }

    /// Pages committed in `[first, first + count)`.
    #[must_use]
    pub fn committed_in(&self, first: usize, count: usize) -> usize {
        let end = first.saturating_add(count).min(self.words.len());
        self.words
            .get(first..end)
            .map_or(0, |s| s.iter().filter(|&&w| matches!(decode(w), PageState::Frame(_))).count())
    }

    /// Every committed frame, in page order (the object's death frees them).
    pub fn frames(&self) -> impl Iterator<Item = u64> + '_ {
        self.words.iter().filter_map(|&w| match decode(w) {
            PageState::Frame(pa) => Some(pa),
            _ => None,
        })
    }

    /// Every frame the store holds (the object's death frees them too).
    pub fn store_frames(&self) -> impl Iterator<Item = u64> + '_ {
        self.store.iter().flat_map(|s| s.frames())
    }

    /// `(records, bytes they occupy, frames the store holds)`.
    #[must_use]
    pub fn store_stats(&self) -> (usize, usize, usize) {
        self.store.as_ref().map_or((0, 0, 0), |s| (s.records(), s.bytes(), s.frames_held()))
    }

    /// A purge (RFC-0100 §4): every committed frame in page order, then every store frame, for
    /// the caller to return to the pool; every page is absent again — zero on its next touch. The
    /// swapped pages' slots are the caller's to tell the pager first (`swapped_slots`).
    pub fn purge(&mut self) -> Vec<u64> {
        let mut frames: Vec<u64> = self.frames().collect();
        frames.extend(self.store_frames());
        for word in self.words.iter_mut() {
            *word = NONE;
        }
        self.committed = 0;
        self.squeezed = 0;
        self.store = None;
        self.swap.clear();
        frames
    }

    /// Pages swapped (their sealed records in the pager's slots).
    #[must_use]
    pub fn swapped(&self) -> usize {
        self.swap.swapped()
    }

    /// The pager's slots of the swapped pages (an object's death or purge tells them).
    pub fn swapped_slots(&self) -> impl Iterator<Item = u32> + '_ {
        self.swap.slots()
    }

    /// Page `index`'s swap entry (exported or swapped).
    #[must_use]
    pub fn swap_entry(&self, index: usize) -> Option<SwapEntry> {
        self.swap.get(u32::try_from(index).ok()?)
    }

    /// Export page `index`'s record under `ctr` (ADR-0088): the page stays a record — and is
    /// inflated as one on a touch — until the pager commits it.
    pub fn export(&mut self, index: usize, ctr: u64) -> Result<(), SwapStateError> {
        let page = u32::try_from(index).map_err(|_| SwapStateError::OutOfRange)?;
        match self.state(index).ok_or(SwapStateError::OutOfRange)? {
            PageState::Record(_) => self.swap.export(page, ctr).map_err(SwapStateError::Swap),
            _ => Err(SwapStateError::NotARecord),
        }
    }

    /// The pager wrote page `index`'s record, sealed under `ctr`, to `slot`: the page is swapped
    /// and its record released. Refused unless the page is still the record of that very export
    /// (a touch cancels an export; a later export records another counter). Answers the store
    /// frames that emptied.
    pub fn swap_commit(
        &mut self,
        index: usize,
        ctr: u64,
        slot: u32,
        mem: &impl PageMemory,
    ) -> Result<Released, SwapStateError> {
        let page = u32::try_from(index).map_err(|_| SwapStateError::OutOfRange)?;
        let record = match self.state(index).ok_or(SwapStateError::OutOfRange)? {
            PageState::Record(record) => record,
            _ => return Err(SwapStateError::NotARecord),
        };
        if self.swap.get(page).is_none_or(|e| e.ctr != ctr || e.slot.is_some()) {
            return Err(SwapStateError::Swap(SwapError::NotExported));
        }
        let store = self.store.as_mut().ok_or(SwapStateError::Store(StoreError::NotARecord))?;
        let released = store.release(record, mem).map_err(SwapStateError::Store)?;
        if store.records() == 0 {
            self.store = None;
        }
        self.swap.commit(page, ctr, slot).map_err(SwapStateError::Swap)?;
        self.words[index] = SWAPPED;
        self.squeezed -= 1;
        Ok(released)
    }

    /// Page `index` came back (ADR-0088): the caller opened the sealed record `entry` names and
    /// decoded it into the frame `pa`; the page holds it and its swap entry is gone. Refused
    /// (`NotSwapped`) when the page is no longer swapped as `entry` says.
    pub fn swap_in(
        &mut self,
        index: usize,
        entry: SwapEntry,
        pa: u64,
    ) -> Result<(), SwapStateError> {
        let word = frame_word(pa).ok_or(SwapStateError::OutOfRange)?;
        if self.state(index).ok_or(SwapStateError::OutOfRange)? != PageState::Swapped
            || self.swap_entry(index) != Some(entry)
        {
            return Err(SwapStateError::NotSwapped);
        }
        self.swap.remove(entry.index);
        self.words[index] = word;
        self.committed += 1;
        Ok(())
    }

    /// Squeeze page `index` (ADR-0087): its content is now `block`, a record in the object's
    /// store. Answers the frame the page held — the caller returns it to the pool once no TLB can
    /// reach it. A store page the record needs takes a frame of `spare`'s.
    pub fn squeeze(
        &mut self,
        index: usize,
        block: &[u8],
        spare: &mut Spare,
        mem: &mut impl PageMemory,
    ) -> Result<u64, SqueezeError> {
        let pa = match self.state(index).ok_or(SqueezeError::OutOfRange)? {
            PageState::Frame(pa) => pa,
            _ => return Err(SqueezeError::NotCommitted),
        };
        let store = self.store.get_or_insert_with(|| Box::new(Store::new()));
        let placed = store.place(block, spare, mem);
        if store.records() == 0 {
            self.store = None;
        }
        let slot = placed.map_err(SqueezeError::Store)?;
        self.words[index] = RECORD | slot;
        self.committed -= 1;
        self.squeezed += 1;
        Ok(pa)
    }

    /// Drop page `index`, a page of zeroes (ADR-0087: a zero page is a fact, not a record): it
    /// is absent again. Answers its frame, for the caller to return as a squeeze's.
    pub fn uncommit(&mut self, index: usize) -> Result<u64, SqueezeError> {
        let pa = match self.state(index).ok_or(SqueezeError::OutOfRange)? {
            PageState::Frame(pa) => pa,
            _ => return Err(SqueezeError::NotCommitted),
        };
        self.words[index] = NONE;
        self.committed -= 1;
        Ok(pa)
    }

    /// Inflate, step one: the block of page `index`'s record copied into `out`. Answers the
    /// record's slot (step two names it) and the block's length.
    pub fn record(
        &self,
        index: usize,
        out: &mut [u8],
        mem: &impl PageMemory,
    ) -> Result<(u32, usize), SqueezeError> {
        let slot = match self.state(index).ok_or(SqueezeError::OutOfRange)? {
            PageState::Record(slot) => slot,
            _ => return Err(SqueezeError::NotSqueezed),
        };
        let store = self.store.as_ref().ok_or(SqueezeError::Store(StoreError::NotARecord))?;
        let len = store.read(slot, out, mem).map_err(SqueezeError::Store)?;
        Ok((slot, len))
    }

    /// Inflate, step two: page `index`, whose record at `slot` the caller decoded into the frame
    /// `pa`, now holds `pa`; the record is released. Answers the store frames that emptied.
    pub fn inflated(
        &mut self,
        index: usize,
        slot: u32,
        pa: u64,
        mem: &impl PageMemory,
    ) -> Result<Released, SqueezeError> {
        let word = frame_word(pa).ok_or(SqueezeError::OutOfRange)?;
        if self.state(index).ok_or(SqueezeError::OutOfRange)? != PageState::Record(slot) {
            return Err(SqueezeError::Changed);
        }
        let store = self.store.as_mut().ok_or(SqueezeError::Store(StoreError::NotARecord))?;
        let released = store.release(slot, mem).map_err(SqueezeError::Store)?;
        if store.records() == 0 {
            self.store = None;
        }
        self.words[index] = word;
        self.squeezed -= 1;
        self.committed += 1;
        // A touch cancels an export the pager has not committed yet (ADR-0088).
        self.swap.remove(index as u32);
        Ok(released)
    }
}

#[cfg(test)]
#[path = "lazy_pages_tests.rs"]
mod tests;
