// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the words of ONE lazy object's page map, sparse (ADR-0093, TASK-0352 P6): a page's
//! word lives in a chunk of `CHUNK` words, and a chunk exists only once a page in it holds
//! something. Reserving an object costs its chunk table — eight bytes per 4 MiB of object — and
//! nothing per page: a 1 GiB object reserved costs 2 KiB of kernel heap, not the 1 MiB of a word
//! per page. The first non-empty word of a chunk's range makes the chunk (above the heap's
//! reserve, ADR-0091, or `NoHeap` to the toucher); a chunk whose words all read `empty` again is
//! freed. What a word means (absent, a frame, a record, swapped) is `lazy_pages.rs`'s; this file
//! knows only the empty word.
//!
//! NOT target-gated (a child of `lazy`, host-proven).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below — a map of 262144 words reserved costs under 64 KiB and a touch
//!   one chunk, a chunk frees when its last word empties, the last chunk is short, `test_reject_*`
//!   (a word past the end, a touch while the pool refuses); `lazy_pages_tests.rs` through the map
//! INVARIANTS: a missing chunk reads `empty` everywhere; `len` never changes; the heap bytes
//!   reported are the chunk table's plus the chunks that exist.

use alloc::boxed::Box;
use alloc::vec::Vec;

/// Words per chunk: 4 KiB of words, 4 MiB of object.
pub(super) const CHUNK: usize = 1024;

/// Why a word was not written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordsError {
    /// `index` lies past the map's end.
    OutOfRange,
    /// The word's chunk does not exist yet and the kernel heap above its reserve refuses it
    /// (ADR-0091): the toucher hears `ENOMEM`.
    NoHeap,
}

type Chunk = Box<[u32]>;

/// `len` words, chunked; a chunk that does not exist reads `empty`.
#[derive(Debug)]
pub struct Words {
    empty: u32,
    len: usize,
    chunks: Vec<Option<Chunk>>,
    present: usize,
}

impl Words {
    /// A map of `len` words, every one `empty` — `None` when the chunk table does not fit above
    /// the heap's reserve (the caller sized the object: the caller hears `ENOMEM`).
    #[must_use]
    pub fn try_new(len: usize, empty: u32) -> Option<Self> {
        let count = len.div_ceil(CHUNK);
        if !crate::kheap::admits(count.checked_mul(core::mem::size_of::<Option<Chunk>>())?) {
            return None;
        }
        let mut chunks = Vec::new();
        chunks.try_reserve_exact(count).ok()?;
        chunks.resize_with(count, || None);
        Some(Self { empty, len, chunks, present: 0 })
    }

    /// Words the map spans.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// The word at `index`; `empty` where no chunk exists; `None` past the end.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<u32> {
        if index >= self.len {
            return None;
        }
        let chunk = self.chunks.get(index / CHUNK)?.as_ref();
        Some(chunk.map_or(self.empty, |chunk| chunk[index % CHUNK]))
    }

    /// Writes `word` at `index`: the first non-empty word of a chunk's range makes the chunk, the
    /// last one emptied frees it. Refused past the end, or when a new chunk finds no heap.
    pub fn set(&mut self, index: usize, word: u32) -> Result<(), WordsError> {
        if index >= self.len {
            return Err(WordsError::OutOfRange);
        }
        let (at, within) = (index / CHUNK, index % CHUNK);
        match self.chunks[at].as_mut() {
            Some(chunk) => chunk[within] = word,
            None if word == self.empty => return Ok(()),
            None => {
                let words = CHUNK.min(self.len - at * CHUNK);
                if !crate::kheap::admits(words * core::mem::size_of::<u32>()) {
                    return Err(WordsError::NoHeap);
                }
                let mut chunk = Vec::new();
                chunk.try_reserve_exact(words).map_err(|_| WordsError::NoHeap)?;
                chunk.resize(words, self.empty);
                chunk[within] = word;
                self.chunks[at] = Some(chunk.into_boxed_slice());
                self.present += 1;
                return Ok(());
            }
        }
        if word == self.empty {
            self.settle(at);
        }
        Ok(())
    }

    /// Replaces the word at `index`, whose chunk exists because the word there is not `empty` —
    /// the callers' invariant (a frame, a record, a swapped page all have a chunk). A missing
    /// chunk or an index past the end is a kernel bug: the write is dropped.
    pub fn replace(&mut self, index: usize, word: u32) {
        let at = index / CHUNK;
        if let Some(chunk) = self.chunks.get_mut(at).and_then(Option::as_mut) {
            chunk[index % CHUNK] = word;
            if word == self.empty {
                self.settle(at);
            }
        }
    }

    /// Frees chunk `at` when every word of it is `empty`.
    fn settle(&mut self, at: usize) {
        let empty = self.empty;
        if self.chunks[at].as_ref().is_some_and(|chunk| chunk.iter().all(|&w| w == empty)) {
            self.chunks[at] = None;
            self.present -= 1;
        }
    }

    /// Every word that is not `empty`, with its index, in order.
    pub fn iter(&self) -> impl Iterator<Item = (usize, u32)> + '_ {
        let empty = self.empty;
        self.chunks.iter().enumerate().flat_map(move |(at, chunk)| {
            chunk.iter().flat_map(move |chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .filter(move |&(_, &w)| w != empty)
                    .map(move |(within, &w)| (at * CHUNK + within, w))
            })
        })
    }

    /// Every word `empty` again; the chunks are gone.
    pub fn clear(&mut self) {
        for chunk in self.chunks.iter_mut() {
            *chunk = None;
        }
        self.present = 0;
    }

    /// Chunks that exist (the tests' count; the kernel reads `heap_bytes`).
    #[cfg(test)]
    #[must_use]
    pub fn chunks(&self) -> usize {
        self.present
    }

    /// Kernel heap bytes the map holds: the chunk table and the chunks that exist.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        let table = self.chunks.capacity() * core::mem::size_of::<Option<Chunk>>();
        let chunks: usize = self.chunks.iter().flatten().map(|chunk| chunk.len()).sum();
        table + chunks * core::mem::size_of::<u32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: u32 = u32::MAX;

    #[test]
    fn reserving_costs_the_chunk_table_and_a_touch_costs_one_chunk() {
        let mut words = Words::try_new(1 << 18, EMPTY).unwrap();
        let table = words.heap_bytes();
        assert!(table <= 64 * 1024, "262144 words reserved cost {table} bytes");
        assert_eq!(words.len(), 1 << 18);
        assert_eq!(words.get(5), Some(EMPTY), "a missing chunk reads empty");
        assert_eq!(words.chunks(), 0);
        words.set(5, 7).unwrap();
        words.set(1000, 8).unwrap();
        assert_eq!(words.heap_bytes(), table + CHUNK * 4, "two words of one chunk: one chunk");
        words.set(1 << 17, 9).unwrap();
        assert_eq!(words.heap_bytes(), table + 2 * CHUNK * 4);
        assert_eq!(words.chunks(), 2);
        assert_eq!(words.iter().collect::<Vec<_>>(), [(5, 7), (1000, 8), (1 << 17, 9)]);
        words.set(5, EMPTY).unwrap();
        assert_eq!(words.chunks(), 2, "a chunk with a word left stays");
        words.replace(1000, EMPTY);
        assert_eq!(words.chunks(), 1, "a chunk whose last word emptied is gone");
        assert_eq!(words.get(1000), Some(EMPTY));
        words.clear();
        assert_eq!((words.chunks(), words.heap_bytes()), (0, table));
        assert_eq!(words.iter().count(), 0);
    }

    #[test]
    fn the_last_chunk_is_as_short_as_the_map() {
        let mut words = Words::try_new(3, EMPTY).unwrap();
        let table = words.heap_bytes();
        words.set(2, 1).unwrap();
        assert_eq!(words.heap_bytes(), table + 3 * 4, "three words, not a thousand");
        let mut words = Words::try_new(CHUNK + 1, EMPTY).unwrap();
        let table = words.heap_bytes();
        words.set(CHUNK, 1).unwrap();
        assert_eq!(words.heap_bytes(), table + 4);
        assert_eq!(words.iter().collect::<Vec<_>>(), [(CHUNK, 1)]);
    }

    #[test]
    fn an_empty_write_into_a_missing_chunk_makes_none() {
        let mut words = Words::try_new(8, EMPTY).unwrap();
        words.set(3, EMPTY).unwrap();
        words.replace(3, EMPTY);
        assert_eq!(words.chunks(), 0);
    }

    #[test]
    fn test_reject_a_word_past_the_end() {
        let mut words = Words::try_new(8, EMPTY).unwrap();
        assert_eq!(words.set(8, 1), Err(WordsError::OutOfRange));
        assert_eq!(words.get(8), None);
        words.replace(8, 1);
        assert_eq!(words.chunks(), 0, "a dropped write makes nothing");
    }

    #[test]
    fn test_reject_a_touch_while_the_pool_refuses() {
        let mut words = Words::try_new(2 * CHUNK, EMPTY).unwrap();
        words.set(0, 1).unwrap();
        let refuses = crate::kheap::refusal::PoolRefuses::new();
        assert_eq!(words.set(CHUNK, 2), Err(WordsError::NoHeap), "a new chunk needs the pool");
        words.set(1, 3).unwrap();
        assert!(Words::try_new(8, EMPTY).is_none(), "the chunk table needs the pool");
        drop(refuses);
        words.set(CHUNK, 2).unwrap();
        assert_eq!(words.iter().collect::<Vec<_>>(), [(0, 1), (1, 3), (CHUNK, 2)]);
    }
}
