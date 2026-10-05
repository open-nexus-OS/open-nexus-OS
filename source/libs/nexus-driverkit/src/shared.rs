// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! DMA memory the CPU and a device share for the device's lifetime (TASK-0328 U1, RFC-0099
//! §3) — a controller's rings, contexts and tables.
//!
//! Unlike a [`crate::DmaBuffer`], neither side ever gives the memory up: the device reads its
//! command and transfer rings and writes its event ring while the CPU keeps producing and
//! consuming entries. So the maintenance a non-coherent device needs happens per entry, not
//! per ownership change: after the CPU writes an entry it [`DmaShared::publish`]es it
//! (`clean` — the device reads memory), and before it reads an entry the device may have
//! written it [`DmaShared::observe`]s it (`flush` — the CPU then reads memory, not a stale
//! line). Ranges are rounded out to whole cache blocks of the memory (which starts on a page).
//! The caller keeps ONE rule: no cache block holds bytes of two writers — then a flush never
//! drops a CPU write the device has not seen, and a clean never overwrites a device write
//! (RFC-0099 §3 lays every structure out that way: each is 64-byte aligned on its own blocks).
//! A coherent device costs nothing; a non-coherent device on harts without Zicbom is refused.

use core::ops::Range;

use nexus_abi::{DmaCoherence, DmaRun};

use crate::dma::{CacheOps, DmaError, DmaMemory};

/// Memory both the CPU and a device use for the device's lifetime.
pub struct DmaShared<M: DmaMemory, C: CacheOps> {
    mem: M,
    cache: C,
    /// `Some(block)` when the device needs maintenance.
    block: Option<usize>,
}

impl<M: DmaMemory, C: CacheOps> DmaShared<M, C> {
    /// Wrap `mem` (page-aligned, made for the device) for a device of the given coherence.
    pub fn new(mem: M, coherence: DmaCoherence, cache: C) -> Result<Self, DmaError> {
        let block = match coherence {
            DmaCoherence::Coherent => None,
            DmaCoherence::Maintained { block } => Some(block),
            DmaCoherence::Unmaintainable => return Err(DmaError::Unmaintainable),
        };
        Ok(Self { mem, cache, block })
    }

    /// The CPU's view (what the device wrote shows only after [`Self::observe`]).
    pub fn bytes(&self) -> &[u8] {
        self.mem.bytes()
    }

    /// The CPU's view, writable (the device sees a write only after [`Self::publish`]).
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        self.mem.bytes_mut()
    }

    /// What the device is programmed with.
    pub fn runs(&self) -> &[DmaRun] {
        self.mem.runs()
    }

    /// Bytes.
    pub fn len(&self) -> usize {
        self.mem.bytes().len()
    }

    /// True for empty memory.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The cache block no two writers may share, `None` for a coherent device.
    pub fn block(&self) -> Option<usize> {
        self.block
    }

    /// The CPU wrote `range`: write it back so the device reads it.
    pub fn publish(&mut self, range: Range<usize>) {
        if let Some((block, span)) = self.span(range) {
            self.cache.clean(&self.mem.bytes()[span], block);
        }
    }

    /// The device may have written `range`: drop the CPU's lines so the next read is memory's.
    pub fn observe(&mut self, range: Range<usize>) {
        if let Some((block, span)) = self.span(range) {
            self.cache.flush(&mut self.mem.bytes_mut()[span], block);
        }
    }

    /// `range` rounded out to whole blocks and clamped to the memory; `None` when there is
    /// nothing to maintain.
    fn span(&self, range: Range<usize>) -> Option<(usize, Range<usize>)> {
        let block = self.block?;
        let len = self.len();
        let end = range.end.min(len);
        if range.start >= end {
            return None;
        }
        Some((block, range.start & !(block - 1)..end.next_multiple_of(block).min(len)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::vec::Vec;

    struct Mem(Vec<u8>, Vec<DmaRun>);

    impl DmaMemory for Mem {
        fn bytes(&self) -> &[u8] {
            &self.0
        }
        fn bytes_mut(&mut self) -> &mut [u8] {
            &mut self.0
        }
        fn runs(&self) -> &[DmaRun] {
            &self.1
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Op {
        /// (pointer, bytes)
        Clean(usize, usize),
        Flush(usize, usize),
    }

    #[derive(Default)]
    struct Recorder(RefCell<Vec<Op>>);

    impl CacheOps for &Recorder {
        fn clean(&self, bytes: &[u8], block: usize) {
            assert_eq!(block, 64);
            self.0.borrow_mut().push(Op::Clean(bytes.as_ptr() as usize, bytes.len()));
        }
        fn flush(&self, bytes: &mut [u8], block: usize) {
            assert_eq!(block, 64);
            self.0.borrow_mut().push(Op::Flush(bytes.as_ptr() as usize, bytes.len()));
        }
    }

    fn shared(rec: &Recorder, coherence: DmaCoherence) -> DmaShared<Mem, &Recorder> {
        let runs = vec![DmaRun { bus: 0x9000_0000, len: 300 }];
        DmaShared::new(Mem(vec![0; 300], runs), coherence, rec).unwrap()
    }

    const MAINTAINED: DmaCoherence = DmaCoherence::Maintained { block: 64 };

    #[test]
    fn a_coherent_device_costs_nothing() {
        let rec = Recorder::default();
        let mut s = shared(&rec, DmaCoherence::Coherent);
        s.bytes_mut()[0] = 1;
        s.publish(0..16);
        s.observe(0..300);
        assert!(rec.0.borrow().is_empty());
        assert_eq!((s.block(), s.len(), s.runs()[0].bus), (None, 300, 0x9000_0000));
    }

    #[test]
    fn entries_are_maintained_in_whole_blocks_clamped_to_the_memory() {
        let rec = Recorder::default();
        let mut s = shared(&rec, MAINTAINED);
        let base = s.bytes().as_ptr() as usize;
        s.publish(16..32); // one 16-byte entry in block 0
        s.publish(48..80); // straddles blocks 0 and 1
        s.observe(128..144); // an event in block 2
        s.observe(290..400); // past the end: clamped to the last, partial block
        s.publish(10..10); // empty
        s.observe(400..500); // wholly past the end
        assert_eq!(
            *rec.0.borrow(),
            [
                Op::Clean(base, 64),
                Op::Clean(base, 128),
                Op::Flush(base + 128, 64),
                Op::Flush(base + 256, 44),
            ]
        );
        assert_eq!(s.block(), Some(64));
    }

    #[test]
    fn test_reject_shared_memory_for_an_unmaintainable_device() {
        let rec = Recorder::default();
        let runs = vec![DmaRun { bus: 0, len: 64 }];
        let refused =
            DmaShared::new(Mem(vec![0; 64], runs), DmaCoherence::Unmaintainable, &rec).err();
        assert_eq!(refused, Some(DmaError::Unmaintainable));
    }
}
