// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the compressed store of ONE lazy object (RFC-0100 §5, TASK-0333 6f-1, ADR-0087) —
//! where the pages a squeeze took live until a touch brings them back. Slots of 64 bytes, one
//! occupancy word per store page (64 slots) and one word marking where each record starts; a
//! record is a 2-byte length and the block, at most [`MAX_RECORD`] bytes (56 slots), so it
//! spans at most two store pages. A record is placed first-fit from the lowest store page with
//! a free slot — the holes a thaw leaves are filled again before the store grows — and a store
//! page whose last slot went free gives its frame back at once. The store lives with its object
//! and dies with it: records of two objects never share a store page, so a kill frees all of an
//! app's. The bytes are reached through [`PageMemory`] (the kernel: the direct map; the tests: a
//! vector); the frames come from the caller ([`Spare`], taken from the pool before the object
//! table's lock and returned after it). Pure index logic, host-proven.
//!
//! NOT target-gated (a child of `lazy`).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6f-1)
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below — round trips across a store-page boundary, first-fit reuse
//!   of a hole, a store page given back when its last record goes, the frames a record needs,
//!   the bounds (`test_reject_*`: an empty or oversized record, a record without its frame, a
//!   slot that starts no record, a record whose length disagrees with its slots)
//! INVARIANTS: a slot belongs to at most one record; a store page holds a frame exactly while
//!   one of its slots is taken; a record is read back only from the slots its start bit and the
//!   occupancy words say are its own.

use alloc::vec::Vec;

use super::PAGE;

/// One slot.
pub const SLOT: usize = 64;
/// Slots per store page: one occupancy word.
pub const SLOTS_PER_PAGE: usize = PAGE / SLOT;
/// A record's length field.
pub const HEADER: usize = 2;
/// The largest record the store takes (7/8 of a page): a page whose block is longer stays a page
/// — the store would gain less than an eighth of it.
pub const MAX_RECORD: usize = PAGE / 8 * 7;
/// The largest block a record carries.
pub const MAX_BLOCK: usize = MAX_RECORD - HEADER;
/// Slot indices a lazy page's word can carry (31 bits, ADR-0087).
pub const MAX_SLOTS: usize = 0x7FFF_FFFE;

const _: () = assert!(SLOTS_PER_PAGE == 64, "one occupancy word per store page");
const _: () = assert!(MAX_RECORD.div_ceil(SLOT) <= SLOTS_PER_PAGE, "a record spans <= 2 pages");

/// The bytes behind the store's frames.
pub trait PageMemory {
    /// Copy `out.len()` bytes of the frame at `pa`, from its byte `at`, into `out`.
    fn read(&self, pa: u64, at: usize, out: &mut [u8]);
    /// Copy `bytes` into the frame at `pa` from its byte `at`.
    fn write(&mut self, pa: u64, at: usize, bytes: &[u8]);
}

/// Frames the caller took from the pool before the table lock. A record lands in at most two
/// store pages, so two are always enough; what a record does not take goes back to the pool.
#[derive(Debug, Default)]
pub struct Spare {
    frames: [Option<u64>; 2],
}

impl Spare {
    /// No frame yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { frames: [None, None] }
    }

    /// Frames held.
    #[must_use]
    pub fn held(&self) -> usize {
        self.frames.iter().flatten().count()
    }

    /// Room for frames (what the caller may still bring).
    #[must_use]
    pub fn missing(&self) -> usize {
        self.frames.len() - self.held()
    }

    /// Bring the frame `pa`; refused (handed back) when two are held.
    pub fn put(&mut self, pa: u64) -> Result<(), u64> {
        match self.frames.iter_mut().find(|f| f.is_none()) {
            Some(slot) => {
                *slot = Some(pa);
                Ok(())
            }
            None => Err(pa),
        }
    }

    fn take(&mut self) -> Option<u64> {
        self.frames.iter_mut().find_map(Option::take)
    }

    /// Every frame still held, for the caller to return to the pool.
    pub fn drain(&mut self) -> impl Iterator<Item = u64> + '_ {
        self.frames.iter_mut().filter_map(Option::take)
    }
}

/// Why a store call was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// An empty block, or a record over [`MAX_RECORD`].
    TooLarge,
    /// The record needs a store page's frame the caller did not bring.
    NoFrame,
    /// The record's first slot would not fit a lazy page's word.
    Full,
    /// The slot starts no record of this store, or the record's length disagrees with its slots.
    NotARecord,
}

/// The frames of the store pages a release emptied (at most the two a record spans).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Released {
    frames: [Option<u64>; 2],
}

impl Released {
    fn push(&mut self, pa: u64) {
        if let Some(slot) = self.frames.iter_mut().find(|f| f.is_none()) {
            *slot = Some(pa);
        }
    }

    /// The frames, for the caller to return to the pool once the table lock is dropped.
    pub fn frames(&self) -> impl Iterator<Item = u64> + '_ {
        self.frames.iter().flatten().copied()
    }
}

/// One store page: its frame and its slots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StorePage {
    /// The frame behind the page; meaningful only while a slot is taken.
    frame: u64,
    /// Bit `i`: slot `i` is taken.
    taken: u64,
    /// Bit `i`: a record starts at slot `i`.
    starts: u64,
}

/// The compressed store of one lazy object.
#[derive(Debug, Default)]
pub struct Store {
    pages: Vec<StorePage>,
    /// Slots taken.
    used: usize,
    /// Records held.
    records: usize,
    /// No store page below this one has a free slot: where first-fit starts.
    low: usize,
}

impl Store {
    /// An empty store.
    #[must_use]
    pub const fn new() -> Self {
        Self { pages: Vec::new(), used: 0, records: 0, low: 0 }
    }

    /// Records held.
    #[must_use]
    pub fn records(&self) -> usize {
        self.records
    }

    /// Bytes the records occupy (whole slots).
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.used * SLOT
    }

    /// Frames the store holds.
    #[must_use]
    pub fn frames_held(&self) -> usize {
        self.pages.iter().filter(|p| p.taken != 0).count()
    }

    /// Every frame the store holds (the object's death or purge returns them).
    pub fn frames(&self) -> impl Iterator<Item = u64> + '_ {
        self.pages.iter().filter(|p| p.taken != 0).map(|p| p.frame)
    }

    /// Put `block` in the store as one record; answers its first slot. A store page the record
    /// lands in that holds no frame yet takes one of `spare`'s — refused (nothing changed) when
    /// `spare` holds too few.
    pub fn place(
        &mut self,
        block: &[u8],
        spare: &mut Spare,
        mem: &mut impl PageMemory,
    ) -> Result<u32, StoreError> {
        let len = HEADER + block.len();
        if block.is_empty() || len > MAX_RECORD {
            return Err(StoreError::TooLarge);
        }
        let k = len.div_ceil(SLOT);
        let first = self.find(k);
        let last = first + k - 1;
        if first > MAX_SLOTS {
            return Err(StoreError::Full);
        }
        let (p0, p1) = (first / SLOTS_PER_PAGE, last / SLOTS_PER_PAGE);
        if (p0..=p1).filter(|&p| self.word(p) == 0).count() > spare.held() {
            return Err(StoreError::NoFrame);
        }
        if self.pages.len() <= p1 {
            self.pages.resize(p1 + 1, StorePage::default());
        }
        for page in &mut self.pages[p0..=p1] {
            if page.taken == 0 {
                // Counted above: `spare` holds a frame for every empty page of the run.
                page.frame = spare.take().unwrap_or_default();
            }
        }
        for s in first..=last {
            self.pages[s / SLOTS_PER_PAGE].taken |= 1 << (s % SLOTS_PER_PAGE);
        }
        self.pages[p0].starts |= 1 << (first % SLOTS_PER_PAGE);
        // A record's length fits its field: `len <= MAX_RECORD < 65536`.
        self.write_at(first * SLOT, &(block.len() as u16).to_le_bytes(), mem);
        self.write_at(first * SLOT + HEADER, block, mem);
        self.used += k;
        self.records += 1;
        while self.pages.get(self.low).is_some_and(|p| p.taken == u64::MAX) {
            self.low += 1;
        }
        Ok(first as u32)
    }

    /// Copy the block of the record at `slot` into `out`; answers its length.
    pub fn read(
        &self,
        slot: u32,
        out: &mut [u8],
        mem: &impl PageMemory,
    ) -> Result<usize, StoreError> {
        let len = self.block_len(slot, mem)?;
        let out = out.get_mut(..len).ok_or(StoreError::TooLarge)?;
        self.read_at(slot as usize * SLOT + HEADER, out, mem);
        Ok(len)
    }

    /// Free the record at `slot`; the store pages it emptied give their frames back.
    pub fn release(&mut self, slot: u32, mem: &impl PageMemory) -> Result<Released, StoreError> {
        let len = self.block_len(slot, mem)?;
        let first = slot as usize;
        let k = (HEADER + len).div_ceil(SLOT);
        for s in first..first + k {
            self.pages[s / SLOTS_PER_PAGE].taken &= !(1 << (s % SLOTS_PER_PAGE));
        }
        self.pages[first / SLOTS_PER_PAGE].starts &= !(1 << (first % SLOTS_PER_PAGE));
        let mut released = Released::default();
        for page in &mut self.pages[first / SLOTS_PER_PAGE..=(first + k - 1) / SLOTS_PER_PAGE] {
            if page.taken == 0 {
                released.push(page.frame);
                page.frame = 0;
            }
        }
        self.used -= k;
        self.records -= 1;
        while self.pages.last().is_some_and(|p| p.taken == 0) {
            self.pages.pop();
        }
        self.low = self.low.min(first / SLOTS_PER_PAGE).min(self.pages.len());
        Ok(released)
    }

    /// The occupancy word of store page `p` (a page past the end is free).
    fn word(&self, p: usize) -> u64 {
        self.pages.get(p).map_or(0, |page| page.taken)
    }

    fn taken(&self, s: usize) -> bool {
        self.word(s / SLOTS_PER_PAGE) & (1 << (s % SLOTS_PER_PAGE)) != 0
    }

    /// The first slot of the lowest run of `k` free slots (`k <= 56`); a run may continue into
    /// the next store page, and pages past the end are free, so there always is one.
    fn find(&self, k: usize) -> usize {
        let mut p = self.low;
        loop {
            let window = u128::from(self.word(p)) | (u128::from(self.word(p + 1)) << 64);
            let free = !window;
            // Bit `i` of `run`: slots `i .. i + k` of the window are free (`i + k <= 119`).
            let mut run = free;
            for shift in 1..k {
                run &= free >> shift;
            }
            let starts = run & u128::from(u64::MAX);
            if starts != 0 {
                return p * SLOTS_PER_PAGE + starts.trailing_zeros() as usize;
            }
            p += 1;
        }
    }

    /// The length of the block of the record at `slot`, checked against the start bit and the
    /// occupancy words.
    fn block_len(&self, slot: u32, mem: &impl PageMemory) -> Result<usize, StoreError> {
        let first = slot as usize;
        let starts = self.pages.get(first / SLOTS_PER_PAGE).map_or(0, |p| p.starts);
        if starts & (1 << (first % SLOTS_PER_PAGE)) == 0 {
            return Err(StoreError::NotARecord);
        }
        let mut field = [0u8; HEADER];
        self.read_at(first * SLOT, &mut field, mem);
        let len = usize::from(u16::from_le_bytes(field));
        if len == 0 || HEADER + len > MAX_RECORD {
            return Err(StoreError::NotARecord);
        }
        let k = (HEADER + len).div_ceil(SLOT);
        if !(first..first + k).all(|s| self.taken(s)) {
            return Err(StoreError::NotARecord);
        }
        Ok(len)
    }

    /// Write `bytes` from store byte `at` (across a store-page boundary when it falls inside).
    fn write_at(&self, at: usize, bytes: &[u8], mem: &mut impl PageMemory) {
        let mut done = 0;
        while done < bytes.len() {
            let pos = at + done;
            let n = (PAGE - pos % PAGE).min(bytes.len() - done);
            mem.write(self.pages[pos / PAGE].frame, pos % PAGE, &bytes[done..done + n]);
            done += n;
        }
    }

    /// Read `out.len()` bytes from store byte `at`.
    fn read_at(&self, at: usize, out: &mut [u8], mem: &impl PageMemory) {
        let mut done = 0;
        while done < out.len() {
            let pos = at + done;
            let n = (PAGE - pos % PAGE).min(out.len() - done);
            mem.read(self.pages[pos / PAGE].frame, pos % PAGE, &mut out[done..done + n]);
            done += n;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Frames as vectors: the pool hands out fake physical addresses one page apart.
    #[derive(Default)]
    pub(crate) struct Frames {
        pages: BTreeMap<u64, Vec<u8>>,
        next: u64,
    }

    impl Frames {
        pub(crate) fn alloc(&mut self) -> u64 {
            self.next += 1;
            let pa = 0x8000_0000 + self.next * PAGE as u64;
            self.pages.insert(pa, vec![0xEE; PAGE]);
            pa
        }

        pub(crate) fn spare(&mut self) -> Spare {
            let mut spare = Spare::new();
            while spare.missing() > 0 {
                let pa = self.alloc();
                assert!(spare.put(pa).is_ok());
            }
            spare
        }
    }

    impl PageMemory for Frames {
        fn read(&self, pa: u64, at: usize, out: &mut [u8]) {
            out.copy_from_slice(&self.pages[&pa][at..at + out.len()]);
        }

        fn write(&mut self, pa: u64, at: usize, bytes: &[u8]) {
            self.pages.get_mut(&pa).expect("a frame the pool handed out")[at..at + bytes.len()]
                .copy_from_slice(bytes);
        }
    }

    fn block(len: usize, seed: u8) -> Vec<u8> {
        (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
    }

    fn read_back(store: &Store, slot: u32, mem: &Frames) -> Vec<u8> {
        let mut out = vec![0u8; MAX_BLOCK];
        let n = store.read(slot, &mut out, mem).expect("a record");
        out.truncate(n);
        out
    }

    #[test]
    fn records_round_trip_and_the_slots_add_up() {
        let mut mem = Frames::default();
        let mut store = Store::new();
        let mut spare = mem.spare();
        let a = store.place(&block(100, 1), &mut spare, &mut mem).unwrap();
        let b = store.place(&block(62, 2), &mut spare, &mut mem).unwrap();
        assert_eq!((a, b), (0, 2), "102 bytes take two slots, 64 take one");
        assert_eq!(read_back(&store, a, &mem), block(100, 1));
        assert_eq!(read_back(&store, b, &mem), block(62, 2));
        assert_eq!((store.records(), store.bytes(), store.frames_held()), (2, 3 * SLOT, 1));
        assert_eq!(spare.held(), 1, "one store page took one frame");
    }

    /// A record that starts near the end of a store page continues in the next one, and both
    /// halves come back.
    #[test]
    fn a_record_spans_two_store_pages() {
        let mut mem = Frames::default();
        let mut store = Store::new();
        let mut spare = mem.spare();
        let filler = store.place(&block(MAX_BLOCK, 3), &mut spare, &mut mem).unwrap();
        let mut spare = mem.spare();
        let across = store.place(&block(1000, 4), &mut spare, &mut mem).unwrap();
        assert_eq!((filler, across), (0, 56));
        assert_eq!(store.frames_held(), 2, "the second record crossed into a second store page");
        assert_eq!(read_back(&store, across, &mem), block(1000, 4));
        assert_eq!(read_back(&store, filler, &mem), block(MAX_BLOCK, 3));
    }

    /// The hole a released record leaves is the first place the next record of its size goes.
    #[test]
    fn a_released_hole_is_filled_first() {
        let mut mem = Frames::default();
        let mut store = Store::new();
        let mut slots = Vec::new();
        for i in 0..8u8 {
            let mut spare = mem.spare();
            slots.push(store.place(&block(500, i), &mut spare, &mut mem).unwrap());
        }
        let grown = store.frames_held();
        store.release(slots[2], &mem).unwrap();
        let mut spare = mem.spare();
        let again = store.place(&block(400, 9), &mut spare, &mut mem).unwrap();
        assert_eq!(again, slots[2], "first-fit: the hole, not the end");
        assert_eq!(store.frames_held(), grown);
        assert_eq!(read_back(&store, again, &mem), block(400, 9));
        assert_eq!(read_back(&store, slots[3], &mem), block(500, 3), "the neighbour is intact");
    }

    /// A store page whose last record goes gives its frame back at once; an empty store holds
    /// nothing.
    #[test]
    fn an_emptied_store_page_gives_its_frame_back() {
        let mut mem = Frames::default();
        let mut store = Store::new();
        let mut spare = mem.spare();
        let a = store.place(&block(MAX_BLOCK, 5), &mut spare, &mut mem).unwrap();
        let mut spare = mem.spare();
        let b = store.place(&block(MAX_BLOCK, 6), &mut spare, &mut mem).unwrap();
        assert_eq!(store.frames_held(), 2);
        let first = store.release(a, &mem).unwrap();
        assert_eq!(first.frames().count(), 0, "the first page still holds b's head");
        let second = store.release(b, &mem).unwrap();
        assert_eq!(second.frames().count(), 2, "b was the last record of both pages");
        assert_eq!((store.records(), store.bytes(), store.frames_held()), (0, 0, 0));
        assert_eq!(store.frames().count(), 0);
    }

    #[test]
    fn test_reject_an_empty_or_oversized_record() {
        let mut mem = Frames::default();
        let mut store = Store::new();
        let mut spare = mem.spare();
        assert_eq!(store.place(&[], &mut spare, &mut mem), Err(StoreError::TooLarge));
        assert_eq!(
            store.place(&block(MAX_BLOCK + 1, 0), &mut spare, &mut mem),
            Err(StoreError::TooLarge)
        );
        assert_eq!((store.records(), spare.held()), (0, 2), "nothing taken");
    }

    /// A record whose store page has no frame and no spare to take is refused whole.
    #[test]
    fn test_reject_a_record_without_its_frame() {
        let mut mem = Frames::default();
        let mut store = Store::new();
        let mut none = Spare::new();
        assert_eq!(store.place(&block(10, 0), &mut none, &mut mem), Err(StoreError::NoFrame));
        assert_eq!((store.records(), store.frames_held()), (0, 0), "refused whole");
        let mut spare = mem.spare();
        store.place(&block(MAX_BLOCK, 1), &mut spare, &mut mem).unwrap();
        // The page's free tail takes a small record without a new frame …
        assert!(store.place(&block(200, 2), &mut none, &mut mem).is_ok());
        // … a record that would cross into a new store page needs one.
        assert_eq!(store.place(&block(1000, 3), &mut none, &mut mem), Err(StoreError::NoFrame));
        assert_eq!((store.records(), store.frames_held()), (2, 1));
    }

    /// A slot that starts no record — inside another record, past the end, released — is never
    /// read back or released.
    #[test]
    fn test_reject_a_slot_that_starts_no_record() {
        let mut mem = Frames::default();
        let mut store = Store::new();
        let mut spare = mem.spare();
        let a = store.place(&block(300, 7), &mut spare, &mut mem).unwrap();
        let mut out = vec![0u8; MAX_BLOCK];
        assert_eq!(store.read(a + 1, &mut out, &mem), Err(StoreError::NotARecord), "inside");
        assert_eq!(store.read(4096, &mut out, &mem), Err(StoreError::NotARecord), "past the end");
        assert_eq!(store.release(a + 2, &mem), Err(StoreError::NotARecord));
        store.release(a, &mem).unwrap();
        assert_eq!(store.read(a, &mut out, &mem), Err(StoreError::NotARecord), "released");
        assert_eq!(store.release(a, &mem), Err(StoreError::NotARecord), "a double release");
    }

    /// A length field that disagrees with the record's slots (memory the store does not own
    /// changed) is refused, never followed.
    #[test]
    fn test_reject_a_record_whose_length_disagrees_with_its_slots() {
        let mut mem = Frames::default();
        let mut store = Store::new();
        let mut spare = mem.spare();
        let a = store.place(&block(60, 1), &mut spare, &mut mem).unwrap();
        let frame = store.frames().next().unwrap();
        mem.write(frame, 0, &2000u16.to_le_bytes());
        let mut out = vec![0u8; MAX_BLOCK];
        assert_eq!(store.read(a, &mut out, &mem), Err(StoreError::NotARecord));
        mem.write(frame, 0, &0u16.to_le_bytes());
        assert_eq!(store.read(a, &mut out, &mem), Err(StoreError::NotARecord), "zero length");
        mem.write(frame, 0, &60u16.to_le_bytes());
        assert_eq!(store.read(a, &mut out, &mem), Ok(60), "restored, it reads again");
    }

    /// Many records of mixed sizes, half released in a scattered order and placed again: every
    /// live record reads back, and the store never holds more pages than its slots need plus
    /// the fragments.
    #[test]
    fn a_churned_store_keeps_every_live_record() {
        let mut mem = Frames::default();
        let mut store = Store::new();
        let mut live: Vec<(u32, Vec<u8>)> = Vec::new();
        for round in 0..4u8 {
            for i in 0..40usize {
                let bytes = block(60 + (i * 97 + usize::from(round) * 13) % 3000, round ^ i as u8);
                let mut spare = mem.spare();
                let slot = store.place(&bytes, &mut spare, &mut mem).unwrap();
                live.push((slot, bytes));
            }
            let mut kept = Vec::new();
            for (n, (slot, bytes)) in live.drain(..).enumerate() {
                if (n * 7 + usize::from(round)) % 3 == 0 {
                    store.release(slot, &mem).unwrap();
                } else {
                    kept.push((slot, bytes));
                }
            }
            live = kept;
            for (slot, bytes) in &live {
                assert_eq!(&read_back(&store, *slot, &mem), bytes);
            }
        }
        assert_eq!(store.records(), live.len());
        let needed = store.bytes().div_ceil(PAGE);
        assert!(store.frames_held() <= needed * 2 + 2, "holes are reused, not leaked");
        // Every record released: nothing held, every frame given back exactly once.
        let mut given_back = Vec::new();
        for (slot, _) in live.drain(..) {
            given_back.extend(store.release(slot, &mem).unwrap().frames());
        }
        assert_eq!((store.records(), store.bytes(), store.frames_held()), (0, 0, 0));
        let mut unique = given_back.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), given_back.len(), "no frame given back twice");
    }
}
