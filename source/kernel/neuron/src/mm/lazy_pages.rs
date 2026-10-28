// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the page table of ONE lazy object (ADR-0077, RFC-0100 §2 and §5) — one word per 4 KiB
//! page: absent (never touched, or purged, or dropped as zeroes: the next touch commits a zeroed
//! frame), a frame, or a RECORD in the object's compressed store (TASK-0333 6f-1, ADR-0087: the
//! word tagged, so no path can mistake a squeezed page for an untouched one). A page is
//! committed once; a squeeze turns a frame into a record and hands the frame back to the caller
//! (who returns it to the pool once no TLB can reach it); an inflate turns the record back into
//! a frame in two steps — the record copied out, decoded by the caller outside the table lock,
//! then entered — so the decode never runs under the lock. Split out of `lazy.rs` (the rules)
//! with the store beside it (`lazy_store.rs`).
//!
//! NOT target-gated (a child of `lazy`).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6b-1; the record state 6f-1)
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below — a page committed once, counts, bounds, the purge; a page
//!   squeezed and brought back byte-exact, a zero page dropped, the store gone with its last
//!   record (`test_reject_*`: a commit over a record, a squeeze of a page that holds no frame, an
//!   inflate of a page whose word moved on)
//! INVARIANTS: a page has at most one frame or one record, never both; a frame number fits 31
//!   bits; `committed` counts the frames, `squeezed` the records.

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::store::{PageMemory, Released, Spare, Store, StoreError};
use super::PAGE;

/// A page's word: never touched (or purged, or dropped as zeroes).
const NONE: u32 = u32::MAX;
/// The tag of a word that names a record: the rest is the record's first slot (ADR-0087).
const RECORD: u32 = 1 << 31;

/// What one page of a lazy object holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageState {
    /// No frame, no record: the next touch commits a zeroed frame.
    Absent,
    /// The page's frame (physical address).
    Frame(u64),
    /// The page lives as the record starting at this slot of the object's store.
    Record(u32),
}

/// Why a commit was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitError {
    /// The page already has this frame: the caller frees the one it brought and uses it.
    Committed(u64),
    /// The page lives as a record: it is inflated, not committed afresh.
    Squeezed,
    /// The page lies past the object's end, or the frame does not fit the table's word.
    OutOfRange,
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
}

fn decode(word: u32) -> PageState {
    if word == NONE {
        PageState::Absent
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
    /// `pages` pages, none committed.
    #[must_use]
    pub fn new(pages: usize) -> Self {
        Self { words: alloc::vec![NONE; pages], committed: 0, squeezed: 0, store: None }
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
    /// the caller to return to the pool; every page is absent again — zero on its next touch.
    pub fn purge(&mut self) -> Vec<u64> {
        let mut frames: Vec<u64> = self.frames().collect();
        frames.extend(self.store_frames());
        for word in self.words.iter_mut() {
            *word = NONE;
        }
        self.committed = 0;
        self.squeezed = 0;
        self.store = None;
        frames
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
        Ok(released)
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::tests::Frames;
    use super::super::store::MAX_BLOCK;
    use super::*;

    #[test]
    fn a_page_is_committed_once_and_keeps_its_frame() {
        let mut pages = LazyPages::new(8);
        assert_eq!(pages.committed(), 0);
        assert_eq!(pages.frame(2), None);
        pages.commit(2, 0x8000_3000).unwrap();
        assert_eq!(pages.frame(2), Some(0x8000_3000));
        assert_eq!(pages.commit(2, 0x8000_9000), Err(CommitError::Committed(0x8000_3000)));
        assert_eq!(pages.committed(), 1, "a refused commit counts nothing");
        pages.commit(7, 0x1_2000_0000).unwrap();
        assert_eq!(pages.committed_in(0, 8), 2);
        assert_eq!(pages.committed_in(3, 4), 0);
        assert_eq!(pages.committed_in(6, 100), 1, "a count past the end is clipped");
        assert_eq!(pages.frames().collect::<Vec<_>>(), [0x8000_3000, 0x1_2000_0000]);
    }

    #[test]
    fn test_reject_a_commit_past_the_end_or_of_a_frame_the_table_cannot_hold() {
        let mut pages = LazyPages::new(4);
        assert_eq!(pages.commit(4, 0x8000_0000), Err(CommitError::OutOfRange));
        assert_eq!(pages.commit(0, 0x8000_0010), Err(CommitError::OutOfRange), "unaligned");
        assert_eq!(pages.commit(0, 1 << 46), Err(CommitError::OutOfRange), "beyond 64 TiB");
        assert_eq!(pages.commit(0, 1 << 43), Err(CommitError::OutOfRange), "the record tag's bit");
        assert_eq!(pages.committed(), 0);
        pages.commit(0, (1 << 43) - PAGE as u64).unwrap();
        assert_eq!(pages.frame(0), Some((1 << 43) - PAGE as u64), "the last frame below 8 TiB");
    }

    /// A purge hands back exactly the committed frames and leaves every page uncommitted; a
    /// second purge hands back nothing; a page committed again counts again.
    #[test]
    fn a_purge_returns_the_committed_frames_and_uncommits_every_page() {
        let mut pages = LazyPages::new(4);
        pages.commit(1, 0x8000_1000).unwrap();
        pages.commit(3, 0x8000_3000).unwrap();
        assert_eq!(pages.purge(), [0x8000_1000, 0x8000_3000]);
        assert_eq!(pages.committed(), 0);
        assert_eq!(pages.frame(1), None);
        assert!(pages.purge().is_empty());
        pages.commit(1, 0x8000_5000).unwrap();
        assert_eq!(pages.committed(), 1);
        assert_eq!(pages.frame(1), Some(0x8000_5000));
    }

    /// The whole way: a page squeezed (its frame handed back, the word a record), read back,
    /// entered into a fresh frame (the record released, the store gone with its last record).
    #[test]
    fn a_squeezed_page_comes_back_into_a_fresh_frame() {
        let mut mem = Frames::default();
        let mut pages = LazyPages::new(4);
        pages.commit(1, 0x9000_1000).unwrap();
        let block: Vec<u8> = (0..900u32).map(|i| (i % 251) as u8).collect();
        let mut spare = mem.spare();
        assert_eq!(pages.squeeze(1, &block, &mut spare, &mut mem), Ok(0x9000_1000));
        assert!(matches!(pages.state(1), Some(PageState::Record(_))));
        assert_eq!((pages.committed(), pages.squeezed()), (0, 1));
        assert_eq!(pages.frame(1), None, "a record is no frame");
        assert_eq!(pages.committed_in(0, 4), 0, "and counts as none");
        assert_eq!(pages.store_stats().2, 1);
        let mut out = vec![0u8; MAX_BLOCK];
        let (slot, len) = pages.record(1, &mut out, &mem).unwrap();
        assert_eq!(&out[..len], &block[..]);
        let released = pages.inflated(1, slot, 0x9000_7000, &mem).unwrap();
        assert_eq!(released.frames().count(), 1, "the store's one page came back");
        assert_eq!(pages.frame(1), Some(0x9000_7000));
        assert_eq!((pages.committed(), pages.squeezed(), pages.store_stats()), (1, 0, (0, 0, 0)));
        assert_eq!(pages.store_frames().count(), 0);
    }

    /// A page of zeroes is dropped, not recorded: absent again, its frame handed back.
    #[test]
    fn a_zero_page_is_dropped() {
        let mut pages = LazyPages::new(2);
        pages.commit(0, 0x9000_0000).unwrap();
        assert_eq!(pages.uncommit(0), Ok(0x9000_0000));
        assert_eq!(pages.state(0), Some(PageState::Absent));
        assert_eq!(pages.committed(), 0);
        assert_eq!(pages.uncommit(0), Err(SqueezeError::NotCommitted));
    }

    /// The store frames go with a purge; the frames of the committed pages first.
    #[test]
    fn a_purge_takes_the_records_and_their_store() {
        let mut mem = Frames::default();
        let mut pages = LazyPages::new(3);
        pages.commit(0, 0x9000_0000).unwrap();
        pages.commit(2, 0x9000_2000).unwrap();
        let mut spare = mem.spare();
        pages.squeeze(2, &[7u8; 300], &mut spare, &mut mem).unwrap();
        let store_frame = pages.store_frames().next().unwrap();
        assert_eq!(pages.purge(), [0x9000_0000, store_frame]);
        assert_eq!((pages.committed(), pages.squeezed(), pages.store_stats()), (0, 0, (0, 0, 0)));
        assert_eq!(pages.state(2), Some(PageState::Absent));
    }

    /// A commit never overwrites a record — the content would be lost and the record leaked.
    #[test]
    fn test_reject_a_commit_over_a_record() {
        let mut mem = Frames::default();
        let mut pages = LazyPages::new(1);
        pages.commit(0, 0x9000_0000).unwrap();
        let mut spare = mem.spare();
        pages.squeeze(0, &[1u8; 64], &mut spare, &mut mem).unwrap();
        assert_eq!(pages.commit(0, 0x9000_5000), Err(CommitError::Squeezed));
        assert_eq!(pages.squeezed(), 1);
    }

    /// Only a committed page is squeezed; a squeeze the store refuses changes nothing.
    #[test]
    fn test_reject_a_squeeze_of_a_page_without_a_frame() {
        let mut mem = Frames::default();
        let mut pages = LazyPages::new(2);
        let mut spare = mem.spare();
        assert_eq!(
            pages.squeeze(0, &[1u8; 64], &mut spare, &mut mem),
            Err(SqueezeError::NotCommitted)
        );
        assert_eq!(
            pages.squeeze(9, &[1u8; 64], &mut spare, &mut mem),
            Err(SqueezeError::OutOfRange)
        );
        pages.commit(1, 0x9000_1000).unwrap();
        let mut none = Spare::new();
        assert_eq!(
            pages.squeeze(1, &[1u8; 64], &mut none, &mut mem),
            Err(SqueezeError::Store(StoreError::NoFrame))
        );
        assert_eq!(pages.frame(1), Some(0x9000_1000), "still the frame");
        assert_eq!((pages.squeezed(), pages.store_stats()), (0, (0, 0, 0)), "no empty store kept");
    }

    /// An inflate whose page moved on between its two steps, or names another record, is
    /// refused and leaves the page as it is.
    #[test]
    fn test_reject_an_inflate_of_a_page_whose_word_moved_on() {
        let mut mem = Frames::default();
        let mut pages = LazyPages::new(2);
        pages.commit(0, 0x9000_0000).unwrap();
        pages.commit(1, 0x9000_1000).unwrap();
        let mut spare = mem.spare();
        pages.squeeze(0, &[3u8; 100], &mut spare, &mut mem).unwrap();
        pages.squeeze(1, &[4u8; 100], &mut spare, &mut mem).unwrap();
        let mut out = vec![0u8; MAX_BLOCK];
        let (slot0, _) = pages.record(0, &mut out, &mem).unwrap();
        let (slot1, _) = pages.record(1, &mut out, &mem).unwrap();
        assert_eq!(pages.inflated(0, slot1, 0x9000_5000, &mem), Err(SqueezeError::Changed));
        pages.inflated(0, slot0, 0x9000_5000, &mem).unwrap();
        assert_eq!(pages.inflated(0, slot0, 0x9000_6000, &mem), Err(SqueezeError::Changed));
        assert_eq!(pages.record(0, &mut out, &mem), Err(SqueezeError::NotSqueezed));
        assert_eq!(pages.frame(0), Some(0x9000_5000));
        assert_eq!(pages.squeezed(), 1, "page 1 is still a record");
        assert_eq!(pages.record(1, &mut out, &mem).map(|r| r.1), Ok(100));
    }
}
