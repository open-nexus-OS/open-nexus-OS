// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the swap side of ONE lazy object's pages (RFC-0100 §5, TASK-0333 6f-2, ADR-0088) —
//! which of its records were EXPORTED to the pager (sealed under which counter) and which are
//! SWAPPED (in which of the pager's slots). Two phases, so nothing is lost: an export leaves the
//! page a record; only the pager's commit makes it swapped, and a touch in between removes the
//! entry (the commit is then refused and the pager frees its slot). The counter is the seal's
//! nonce: the page opens only under the counter recorded here, so an older sealed record of the
//! same page never opens again. Entries sorted by page index (a frozen app swaps hundreds of
//! pages, never millions). Pure, host-proven.
//!
//! NOT target-gated (a child of `lazy`).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6f-2)
//! API_STABILITY: Internal
//! The export buffer's layout (what `swap_export`, syscall 71, writes for the pager) lives here
//! too: a header, then one entry per sealed record — the page's token, the counter it was sealed
//! under (the commit names it back, so a commit can only ever complete the export it answers),
//! the sealed length, the sealed bytes padded to 4.
//!
//! TEST_COVERAGE: unit tests below — export, commit, the slots told, a touch cancelling an export
//!   (`test_reject_*`: a commit without an export, a commit naming another export's counter, the
//!   superseded export, a second commit); the buffer's layout and its short-buffer refusal
//! INVARIANTS: one entry per page; an entry is exported (no slot) or swapped (a slot), never
//!   both; a swapped entry's counter never changes (it is the one its sealed record opens under).

use alloc::vec::Vec;

/// The export buffer's layout version.
pub const EXPORT_VERSION: u32 = 1;
/// The buffer's header: `version: u32`, `count: u32`.
pub const EXPORT_HEADER: usize = 8;
/// An entry's head: `token: u64`, `ctr: u64`, `len: u16`, `pad: u16`.
pub const ENTRY_HEAD: usize = 20;
/// Records one export seals at most (the kernel lock is held for the batch).
pub const EXPORT_BATCH: usize = 16;
/// The smallest buffer an export takes: the header and the largest entry.
pub const EXPORT_MIN_BUF: usize = EXPORT_HEADER + entry_len(super::seal::MAX_SEALED);
/// The most of a buffer one export can fill: the header and a full batch of the largest entries
/// (a longer buffer is used up to here — and only this much of it is committed).
pub const EXPORT_MAX_BUF: usize = EXPORT_HEADER + EXPORT_BATCH * entry_len(super::seal::MAX_SEALED);

/// An entry's bytes for a sealed record of `sealed` bytes.
#[must_use]
pub const fn entry_len(sealed: usize) -> usize {
    ENTRY_HEAD + sealed.next_multiple_of(4)
}

/// A page's token: its object in the high half, its index in the low half.
#[must_use]
pub const fn token(object: u32, index: u32) -> u64 {
    ((object as u64) << 32) | index as u64
}

/// `(object, index)` of a token.
#[must_use]
pub const fn token_parts(token: u64) -> (u32, u32) {
    ((token >> 32) as u32, token as u32)
}

/// The buffer's header for `count` entries.
pub fn write_header(out: &mut [u8], count: u32) -> Option<()> {
    let head = out.get_mut(..EXPORT_HEADER)?;
    head[..4].copy_from_slice(&EXPORT_VERSION.to_le_bytes());
    head[4..].copy_from_slice(&count.to_le_bytes());
    Some(())
}

/// Complete the entry at the start of `entry` whose `sealed` bytes already sit at
/// `entry[ENTRY_HEAD..]` (it is built in place, where the record was sealed): the head written,
/// the pad zeroed. Answers the entry's bytes; `None` (nothing written) when it does not fit or
/// the record is longer than a `u16`.
pub fn finish_entry(entry: &mut [u8], token: u64, ctr: u64, sealed: usize) -> Option<usize> {
    let len = u16::try_from(sealed).ok()?;
    let entry = entry.get_mut(..entry_len(sealed))?;
    entry[0..8].copy_from_slice(&token.to_le_bytes());
    entry[8..16].copy_from_slice(&ctr.to_le_bytes());
    entry[16..18].copy_from_slice(&len.to_le_bytes());
    entry[18..ENTRY_HEAD].fill(0);
    entry[ENTRY_HEAD + sealed..].fill(0);
    Some(entry.len())
}

/// One page's swap state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapEntry {
    /// The page's index in its object.
    pub index: u32,
    /// The seal's counter the record was exported under (its nonce).
    pub ctr: u64,
    /// The pager's slot once committed; `None` while exported only.
    pub slot: Option<u32>,
}

/// Why a swap step was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapError {
    /// The export the pager names is not the one recorded: never made, cancelled by a touch,
    /// or superseded by a later export.
    NotExported,
    /// The page is swapped already: a commit counts once, a swapped page is not exported.
    AlreadySwapped,
}

/// The swap entries of one object, sorted by page index.
#[derive(Debug, Default)]
pub struct SwapPages {
    entries: Vec<SwapEntry>,
}

impl SwapPages {
    /// No entry.
    #[must_use]
    pub const fn new() -> Self {
        Self { entries: Vec::new() }
    }

    fn find(&self, index: u32) -> Result<usize, usize> {
        self.entries.binary_search_by_key(&index, |e| e.index)
    }

    /// Page `index`'s entry.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<SwapEntry> {
        self.find(index).ok().map(|at| self.entries[at])
    }

    /// Record page `index` as exported under `ctr`. An export the pager never committed is
    /// superseded — its commit names the old counter and is refused — so a pager that died
    /// between export and commit never strands a page in the pool.
    pub fn export(&mut self, index: u32, ctr: u64) -> Result<(), SwapError> {
        match self.find(index) {
            Ok(at) if self.entries[at].slot.is_some() => Err(SwapError::AlreadySwapped),
            Ok(at) => {
                self.entries[at].ctr = ctr;
                Ok(())
            }
            Err(at) => {
                self.entries.insert(at, SwapEntry { index, ctr, slot: None });
                Ok(())
            }
        }
    }

    /// The pager wrote page `index`'s record, sealed under `ctr`, to `slot`: the page is swapped.
    /// Refused unless the export recorded is the one the pager names (`ctr`): a touch may have
    /// cancelled it and a later export taken its place.
    pub fn commit(&mut self, index: u32, ctr: u64, slot: u32) -> Result<(), SwapError> {
        let at = self.find(index).map_err(|_| SwapError::NotExported)?;
        let entry = &mut self.entries[at];
        if entry.ctr != ctr {
            return Err(SwapError::NotExported);
        }
        if entry.slot.is_some() {
            return Err(SwapError::AlreadySwapped);
        }
        entry.slot = Some(slot);
        Ok(())
    }

    /// Forget page `index` (a touch cancelled its export, or it came back).
    pub fn remove(&mut self, index: u32) -> Option<SwapEntry> {
        self.find(index).ok().map(|at| self.entries.remove(at))
    }

    /// Pages swapped (committed).
    #[must_use]
    pub fn swapped(&self) -> usize {
        self.entries.iter().filter(|e| e.slot.is_some()).count()
    }

    /// The slots of the swapped pages (an object's death tells them to the pager).
    pub fn slots(&self) -> impl Iterator<Item = u32> + '_ {
        self.entries.iter().filter_map(|e| e.slot)
    }

    /// No entry at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Forget every entry (a purge).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_export_is_committed_once_and_its_slot_is_told() {
        let mut swap = SwapPages::new();
        swap.export(7, 41).unwrap();
        swap.export(2, 42).unwrap();
        assert_eq!(swap.get(7), Some(SwapEntry { index: 7, ctr: 41, slot: None }));
        assert_eq!(swap.swapped(), 0, "exported is not swapped");
        assert_eq!(swap.commit(7, 41, 300), Ok(()));
        assert_eq!(swap.swapped(), 1);
        assert_eq!(swap.slots().collect::<Vec<_>>(), [300]);
        assert_eq!(swap.remove(7).map(|e| e.slot), Some(Some(300)));
        assert_eq!(swap.get(7), None);
        assert_eq!(swap.get(2).map(|e| e.ctr), Some(42), "the neighbour kept");
    }

    /// A touch between export and commit removes the entry: the pager's commit is refused.
    #[test]
    fn test_reject_a_commit_without_an_export() {
        let mut swap = SwapPages::new();
        assert_eq!(swap.commit(1, 5, 9), Err(SwapError::NotExported));
        swap.export(1, 5).unwrap();
        swap.remove(1);
        assert_eq!(swap.commit(1, 5, 9), Err(SwapError::NotExported), "cancelled by a touch");
        assert!(swap.is_empty());
    }

    /// A late commit of an export a touch cancelled must not complete the export that took its
    /// place: its slot holds a record sealed under the old counter, which would never open.
    #[test]
    fn test_reject_a_commit_naming_another_exports_counter() {
        let mut swap = SwapPages::new();
        swap.export(4, 10).unwrap();
        swap.remove(4);
        swap.export(4, 11).unwrap();
        assert_eq!(swap.commit(4, 10, 70), Err(SwapError::NotExported), "the old export's slot");
        assert_eq!(swap.get(4), Some(SwapEntry { index: 4, ctr: 11, slot: None }));
        assert_eq!(swap.commit(4, 11, 71), Ok(()));
    }

    /// A second export supersedes an uncommitted first (the first's commit is refused); a
    /// swapped page is neither exported nor committed again.
    #[test]
    fn test_reject_the_superseded_export_and_a_second_commit() {
        let mut swap = SwapPages::new();
        swap.export(3, 1).unwrap();
        swap.export(3, 2).unwrap();
        assert_eq!(swap.get(3).map(|e| e.ctr), Some(2), "the later export holds");
        assert_eq!(swap.commit(3, 1, 10), Err(SwapError::NotExported), "the superseded one");
        swap.commit(3, 2, 10).unwrap();
        assert_eq!(swap.commit(3, 2, 11), Err(SwapError::AlreadySwapped));
        assert_eq!(swap.get(3).and_then(|e| e.slot), Some(10));
        assert_eq!(swap.export(3, 4), Err(SwapError::AlreadySwapped), "swapped stays swapped");
        assert_eq!(swap.get(3), Some(SwapEntry { index: 3, ctr: 2, slot: Some(10) }));
    }

    /// An entry built in place: the sealed bytes put at `ENTRY_HEAD`, then finished.
    fn entry_at(buf: &mut [u8], token: u64, ctr: u64, sealed: &[u8]) -> Option<usize> {
        buf.get_mut(ENTRY_HEAD..ENTRY_HEAD + sealed.len())?.copy_from_slice(sealed);
        finish_entry(buf, token, ctr, sealed.len())
    }

    /// The pager's view of the buffer: a header, then entries it walks by their own lengths.
    #[test]
    fn the_export_buffer_is_a_header_and_padded_entries() {
        let mut buf = [0xeeu8; 128];
        let first = entry_at(&mut buf[EXPORT_HEADER..], token(3, 9), 77, &[1, 2, 3]).unwrap();
        assert_eq!(first, ENTRY_HEAD + 4, "three bytes padded to four");
        let at = EXPORT_HEADER + first;
        let second = entry_at(&mut buf[at..], token(4, 1), 78, &[9; 8]).unwrap();
        assert_eq!(second, ENTRY_HEAD + 8);
        write_header(&mut buf, 2).unwrap();
        let word = |at: usize| u32::from_le_bytes(buf[at..at + 4].try_into().unwrap());
        assert_eq!((word(0), word(4)), (EXPORT_VERSION, 2));
        let entry = &buf[EXPORT_HEADER..];
        assert_eq!(token_parts(u64::from_le_bytes(entry[0..8].try_into().unwrap())), (3, 9));
        assert_eq!(u64::from_le_bytes(entry[8..16].try_into().unwrap()), 77);
        assert_eq!(u16::from_le_bytes([entry[16], entry[17]]), 3);
        assert_eq!(&entry[18..20], &[0, 0]);
        assert_eq!(&entry[ENTRY_HEAD..ENTRY_HEAD + 4], &[1, 2, 3, 0], "the pad is zero");
        assert_eq!(buf[at + second], 0xee, "nothing past the last entry");
        assert!(EXPORT_MIN_BUF >= EXPORT_HEADER + ENTRY_HEAD + super::super::seal::MAX_SEALED);
        assert_eq!(EXPORT_MAX_BUF, EXPORT_HEADER + EXPORT_BATCH * (EXPORT_MIN_BUF - EXPORT_HEADER));
    }

    #[test]
    fn test_reject_an_entry_that_does_not_fit() {
        let mut tight = [0x11u8; ENTRY_HEAD + 5];
        assert_eq!(finish_entry(&mut tight, 1, 1, 6), None, "six bytes padded to eight");
        assert!(tight.iter().all(|b| *b == 0x11), "nothing written");
        assert_eq!(finish_entry(&mut [0u8; 70_000], 1, 1, 65_536), None, "over a u16");
        assert_eq!(write_header(&mut [0u8; 7], 1), None);
    }

    #[test]
    fn entries_stay_sorted_and_clear_forgets_them() {
        let mut swap = SwapPages::new();
        for (i, index) in [9u32, 1, 5, 3, 7].iter().enumerate() {
            swap.export(*index, i as u64).unwrap();
        }
        for index in [1u32, 3, 5, 7, 9] {
            assert!(swap.get(index).is_some());
        }
        swap.commit(5, 2, 50).unwrap();
        swap.commit(1, 1, 10).unwrap();
        let mut slots: Vec<u32> = swap.slots().collect();
        slots.sort_unstable();
        assert_eq!(slots, [10, 50]);
        swap.clear();
        assert!(swap.is_empty());
    }
}
