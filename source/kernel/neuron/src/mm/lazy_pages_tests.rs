// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the unit tests of ONE lazy object's page table (`lazy_pages.rs`; ADR-0077, the record
//! ADR-0087, the swapped state ADR-0088) — split out so the table stays under the module-size
//! ratchet. The store's frame double (`Frames`) comes from `lazy_store.rs`'s tests.
//!
//! NOT target-gated (a child of `lazy::pages`, host only).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file — a page committed once, counts, bounds, the purge; a page squeezed
//!   and brought back byte-exact, a zero page dropped, the store gone with its last record; a
//!   page exported, swapped and back (`test_reject_*`: a commit over a record, a squeeze of a page
//!   without a frame, an inflate of a page whose word moved on, a swap commit after a touch or
//!   naming another export, a swap step out of order)

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
    assert_eq!(pages.squeeze(0, &[1u8; 64], &mut spare, &mut mem), Err(SqueezeError::NotCommitted));
    assert_eq!(pages.squeeze(9, &[1u8; 64], &mut spare, &mut mem), Err(SqueezeError::OutOfRange));
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

/// A squeezed page with its record in the store, ready to be exported.
fn squeezed_page(mem: &mut Frames) -> LazyPages {
    let mut pages = LazyPages::new(2);
    pages.commit(1, 0x9000_1000).unwrap();
    let mut spare = mem.spare();
    pages.squeeze(1, &[9u8; 500], &mut spare, mem).unwrap();
    pages
}

/// The swap stage's way out and back (ADR-0088): exported (still a record), committed
/// (swapped, the record and its store page released, the slot told), back into a fresh frame.
#[test]
fn a_swapped_page_leaves_its_record_and_comes_back_into_a_frame() {
    let mut mem = Frames::default();
    let mut pages = squeezed_page(&mut mem);
    pages.export(1, 77).unwrap();
    assert!(matches!(pages.state(1), Some(PageState::Record(_))), "exported is a record still");
    let released = pages.swap_commit(1, 77, 412, &mem).unwrap();
    assert_eq!(released.frames().count(), 1, "the store's only page came back");
    assert_eq!(pages.state(1), Some(PageState::Swapped));
    let entry = SwapEntry { index: 1, ctr: 77, slot: Some(412) };
    assert_eq!(pages.swap_entry(1), Some(entry));
    assert_eq!((pages.squeezed(), pages.swapped(), pages.store_stats()), (0, 1, (0, 0, 0)));
    assert_eq!(pages.swapped_slots().collect::<Vec<_>>(), [412]);
    assert_eq!(pages.commit(1, 0x9000_5000), Err(CommitError::Swapped), "never zeroed afresh");
    pages.swap_in(1, entry, 0x9000_6000).unwrap();
    assert_eq!(pages.frame(1), Some(0x9000_6000));
    assert_eq!((pages.committed(), pages.swapped(), pages.swap_entry(1)), (1, 0, None));
}

/// A touch between export and commit inflates the record and cancels the export: the
/// pager's commit is refused and the page keeps its fresh frame.
#[test]
fn test_reject_a_commit_after_a_touch_cancelled_the_export() {
    let mut mem = Frames::default();
    let mut pages = squeezed_page(&mut mem);
    pages.export(1, 5).unwrap();
    let mut out = vec![0u8; MAX_BLOCK];
    let (slot, _) = pages.record(1, &mut out, &mem).unwrap();
    pages.inflated(1, slot, 0x9000_7000, &mem).unwrap();
    assert_eq!(pages.swap_entry(1), None, "the touch cancelled it");
    assert_eq!(pages.swap_commit(1, 5, 3, &mem), Err(SwapStateError::NotARecord));
    assert_eq!(pages.frame(1), Some(0x9000_7000));
}

#[test]
fn test_reject_an_export_or_commit_or_swap_in_out_of_order() {
    let mut mem = Frames::default();
    let mut pages = squeezed_page(&mut mem);
    let not_exported = Err(SwapStateError::Swap(SwapError::NotExported));
    assert_eq!(pages.export(0, 1), Err(SwapStateError::NotARecord), "an absent page");
    assert_eq!(pages.export(9, 1), Err(SwapStateError::OutOfRange));
    assert_eq!(pages.swap_commit(1, 2, 3, &mem), not_exported, "a record never exported");
    let entry = SwapEntry { index: 1, ctr: 2, slot: Some(8) };
    assert_eq!(pages.swap_in(1, entry, 0x9000_8000), Err(SwapStateError::NotSwapped));
    pages.export(1, 1).unwrap();
    pages.export(1, 2).unwrap();
    assert_eq!(pages.swap_commit(1, 1, 8, &mem), not_exported, "the superseded export");
    assert_eq!(pages.swap_commit(1, 99, 8, &mem), not_exported, "another export's counter");
    pages.swap_commit(1, 2, 8, &mem).unwrap();
    assert_eq!(pages.swap_commit(1, 2, 9, &mem), Err(SwapStateError::NotARecord), "swapped once");
    assert_eq!(pages.export(1, 3), Err(SwapStateError::NotARecord), "a swapped page");
    assert_eq!(pages.swap_entry(1).and_then(|e| e.slot), Some(8));
    let other_slot = SwapEntry { slot: Some(9), ..entry };
    assert_eq!(pages.swap_in(1, other_slot, 0x9000_8000), Err(SwapStateError::NotSwapped));
    assert_eq!(pages.state(1), Some(PageState::Swapped), "a refused swap-in changes nothing");
}

/// A purge forgets the swapped pages; their slots are read before it (the pager's to free).
#[test]
fn a_purge_forgets_the_swapped_pages() {
    let mut mem = Frames::default();
    let mut pages = squeezed_page(&mut mem);
    pages.export(1, 4).unwrap();
    pages.swap_commit(1, 4, 21, &mem).unwrap();
    assert_eq!(pages.swapped_slots().collect::<Vec<_>>(), [21]);
    assert!(pages.purge().is_empty(), "no frame: the page was on the pager's disk");
    assert_eq!((pages.swapped(), pages.state(1)), (0, Some(PageState::Absent)));
}

/// ADR-0093 (TASK-0352 P6): reserving a 1 GiB object costs its chunk table, a touch one chunk,
/// and a chunk whose last page emptied is gone.
#[test]
fn reserving_costs_the_chunk_table_and_a_touch_costs_one_chunk() {
    let mut pages = LazyPages::new(1 << 18);
    let table = pages.heap_bytes();
    assert!(table <= 64 * 1024, "1 GiB reserved costs {table} bytes of heap");
    assert_eq!((pages.pages(), pages.committed()), (1 << 18, 0));
    pages.commit(5, 0x8000_0000).unwrap();
    pages.commit(1000, 0x8000_1000).unwrap();
    assert_eq!(pages.heap_bytes(), table + 4096, "two pages of one chunk: one chunk");
    pages.commit(1 << 17, 0x8000_2000).unwrap();
    assert_eq!(pages.heap_bytes(), table + 2 * 4096);
    assert_eq!(pages.frames().collect::<Vec<_>>(), [0x8000_0000, 0x8000_1000, 0x8000_2000]);
    assert_eq!((pages.committed_in(0, 1024), pages.committed_in(1024, 1 << 18)), (2, 1));
    assert_eq!(pages.uncommit(5), Ok(0x8000_0000));
    assert_eq!(pages.uncommit(1000), Ok(0x8000_1000));
    assert_eq!(pages.heap_bytes(), table + 4096, "a chunk whose last page emptied is gone");
    assert_eq!(pages.purge(), [0x8000_2000]);
    assert_eq!(pages.heap_bytes(), table);
}

/// A new chunk needs the heap above its reserve: while the pool refuses, a touch into a missing
/// chunk is `NoHeap`, a touch into an existing one is not, and no object is reserved.
#[test]
fn test_reject_a_touch_while_the_pool_refuses() {
    let mut pages = LazyPages::new(2048);
    pages.commit(0, 0x8000_0000).unwrap();
    let refuses = crate::kheap::refusal::PoolRefuses::new();
    assert_eq!(pages.commit(1500, 0x8000_1000), Err(CommitError::NoHeap));
    assert_eq!(pages.state(1500), Some(PageState::Absent), "a refused commit leaves no trace");
    pages.commit(1, 0x8000_2000).unwrap();
    assert!(LazyPages::try_new(8).is_none(), "the chunk table needs the pool");
    drop(refuses);
    pages.commit(1500, 0x8000_1000).unwrap();
    assert_eq!(pages.committed(), 3);
}
