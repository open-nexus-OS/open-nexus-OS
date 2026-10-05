// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: DMA memory as a non-coherent machine has it, for every host model a driver is
//! proven against (the SDHCI model, TASK-0246; the xHCI model, TASK-0328): the device reads
//! and writes DRAM by bus address ([`Dram`]); the CPU sees its own view ([`ModelMem`]'s bytes)
//! through a write-back cache. The two meet only in the cache instructions ([`ModelCache`]) —
//! `clean` writes the CPU's dirty bytes of a block range back, `flush` writes them back and
//! reloads the range from DRAM — so a driver that skips one hands the device stale memory or
//! reads stale memory itself, and the data shows it. Dirty bytes are the ones that differ from
//! what the view last synced with DRAM. A machine owns its [`Dram`] and says where through
//! [`HasDram`]; every instruction is logged ([`CacheOp`]) for tests that pin the protocol.
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the models' suites (`storage-sdhci`'s `tests/sdhci`, `xhcid`'s tests) and
//!   the unit tests below (sub-range clean/flush, dirty tracking)

#![forbid(unsafe_code)]
// A test machine, host only: a fixture that cannot be built is a failed test, reported where it
// broke — its inputs are the tests' own constants, never untrusted data.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use nexus_abi::DmaRun;
use nexus_driverkit::{CacheOps, DmaMemory};

/// A cache instruction, as the model saw it: `(region, offset, bytes)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheOp {
    /// Write back.
    Clean(usize, usize, usize),
    /// Write back and reload.
    Flush(usize, usize, usize),
}

struct Region {
    /// `(bus, len)` pieces, in CPU order.
    runs: Vec<(u64, usize)>,
    dram: Vec<u8>,
    /// The CPU view as it was at the last sync.
    synced: Vec<u8>,
    /// Where the CPU view lives (identity only, never dereferenced).
    cpu: usize,
}

/// Every region's DRAM, by bus address.
#[derive(Default)]
pub struct Dram {
    regions: Vec<Region>,
    /// The cache instructions in order.
    pub ops: Vec<CacheOp>,
}

/// A machine that owns a [`Dram`].
pub trait HasDram {
    /// The machine's DRAM.
    fn dram(&mut self) -> &mut Dram;
}

impl Dram {
    /// The region holding bus address `bus` and its offset there. Memory is reused: a driver
    /// that drops a buffer and makes a new one may get the same addresses (the heap's for the
    /// CPU view, the allocator's for the bus), so the newest region at an address wins.
    fn locate(&self, bus: u64) -> Option<(usize, usize)> {
        for (id, region) in self.regions.iter().enumerate().rev() {
            let mut offset = 0usize;
            for &(base, len) in &region.runs {
                if base <= bus && bus < base + len as u64 {
                    return Some((id, offset + (bus - base) as usize));
                }
                offset += len;
            }
        }
        None
    }

    /// The device reads `out.len()` bytes at `bus` (one run's worth); `None` outside memory.
    pub fn read(&self, bus: u64, out: &mut [u8]) -> Option<()> {
        let (id, at) = self.locate(bus)?;
        out.copy_from_slice(self.regions[id].dram.get(at..at + out.len())?);
        Some(())
    }

    /// The device writes `data` at `bus`; `None` outside memory.
    pub fn write(&mut self, bus: u64, data: &[u8]) -> Option<()> {
        let (id, at) = self.locate(bus)?;
        self.regions[id].dram.get_mut(at..at + data.len())?.copy_from_slice(data);
        Some(())
    }

    /// The region and offset of a CPU slice the model made (the newest at that address).
    fn by_cpu(&self, bytes: &[u8]) -> (usize, usize) {
        let at = bytes.as_ptr() as usize;
        let id = self
            .regions
            .iter()
            .rposition(|r| r.cpu <= at && at + bytes.len() <= r.cpu + r.dram.len())
            .expect("cache op on memory the model made");
        (id, at - self.regions[id].cpu)
    }

    fn write_back(&mut self, id: usize, offset: usize, bytes: &[u8]) {
        let region = &mut self.regions[id];
        for (i, &b) in bytes.iter().enumerate() {
            if b != region.synced[offset + i] {
                region.dram[offset + i] = b;
                region.synced[offset + i] = b;
            }
        }
    }
}

/// DMA memory for the device: a CPU view and the bus pieces behind it.
pub struct ModelMem {
    cpu: Vec<u8>,
    runs: Vec<DmaRun>,
}

impl ModelMem {
    /// `pieces` bus runs of whole pages, `gap` pages apart, from `bus` on (CPU-contiguous).
    pub fn new<S: HasDram>(
        m: &Rc<RefCell<S>>,
        len: usize,
        bus: u64,
        pieces: usize,
        gap: u64,
    ) -> Self {
        let cpu = vec![0u8; len];
        let each = len.div_ceil(pieces).div_ceil(4096) * 4096;
        let mut runs = Vec::new();
        let (mut left, mut at) = (len, bus);
        while left > 0 {
            let take = left.min(each);
            runs.push((at, take));
            at += take as u64 + gap * 4096;
            left -= take;
        }
        m.borrow_mut().dram().regions.push(Region {
            runs: runs.clone(),
            dram: vec![0; len],
            synced: vec![0; len],
            cpu: cpu.as_ptr() as usize,
        });
        let runs = runs.into_iter().map(|(bus, len)| DmaRun { bus, len: len as u64 }).collect();
        Self { cpu, runs }
    }
}

impl DmaMemory for ModelMem {
    fn bytes(&self) -> &[u8] {
        &self.cpu
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.cpu
    }
    fn runs(&self) -> &[DmaRun] {
        &self.runs
    }
}

/// The cache instructions, acting on the machine's DRAM.
pub struct ModelCache<S>(pub Rc<RefCell<S>>);

impl<S: HasDram> CacheOps for ModelCache<S> {
    fn clean(&self, bytes: &[u8], block: usize) {
        assert_eq!(block, 64, "the harts' block");
        let mut m = self.0.borrow_mut();
        let dram = m.dram();
        let (id, offset) = dram.by_cpu(bytes);
        dram.write_back(id, offset, bytes);
        dram.ops.push(CacheOp::Clean(id, offset, bytes.len()));
    }

    fn flush(&self, bytes: &mut [u8], block: usize) {
        assert_eq!(block, 64, "the harts' block");
        let mut m = self.0.borrow_mut();
        let dram = m.dram();
        let (id, offset) = dram.by_cpu(bytes);
        dram.write_back(id, offset, bytes);
        let region = &mut dram.regions[id];
        let span = offset..offset + bytes.len();
        bytes.copy_from_slice(&region.dram[span.clone()]);
        region.synced[span.clone()].copy_from_slice(&region.dram[span]);
        dram.ops.push(CacheOp::Flush(id, offset, bytes.len()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Machine(Dram);

    impl HasDram for Machine {
        fn dram(&mut self) -> &mut Dram {
            &mut self.0
        }
    }

    #[test]
    fn a_sub_range_is_cleaned_and_flushed_alone() {
        let m = Rc::new(RefCell::new(Machine::default()));
        let mut mem = ModelMem::new(&m, 256, 0x4000_0000, 1, 0);
        let cache = ModelCache(m.clone());
        mem.bytes_mut()[0] = 0x11;
        mem.bytes_mut()[64] = 0x22;
        // Only block 1 is written back: the device sees 0x22, not yet 0x11.
        cache.clean(&mem.bytes()[64..128], 64);
        let mut seen = [0u8; 1];
        m.borrow().0.read(0x4000_0040, &mut seen).unwrap();
        assert_eq!(seen, [0x22]);
        m.borrow().0.read(0x4000_0000, &mut seen).unwrap();
        assert_eq!(seen, [0x00], "block 0 still sits in the cache");
        // The device writes block 2; the CPU sees it only after flushing that block.
        m.borrow_mut().0.write(0x4000_0080, &[0x33]).unwrap();
        assert_eq!(mem.bytes()[128], 0x00);
        cache.flush(&mut mem.bytes_mut()[128..192], 64);
        assert_eq!(mem.bytes()[128], 0x33);
        // A flush writes the CPU's dirty bytes of its range back first.
        cache.flush(&mut mem.bytes_mut()[0..64], 64);
        m.borrow().0.read(0x4000_0000, &mut seen).unwrap();
        assert_eq!((seen, mem.bytes()[0]), ([0x11], 0x11));
        assert_eq!(
            m.borrow().0.ops,
            [CacheOp::Clean(0, 64, 64), CacheOp::Flush(0, 128, 64), CacheOp::Flush(0, 0, 64)]
        );
    }
}
