// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: DMA memory as a non-coherent machine has it: the device reads and writes DRAM by bus
//! address; the CPU sees its own view (`ModelMem`'s bytes) through a write-back cache. The
//! two meet only in the cache instructions — `clean` writes the CPU's dirty bytes back,
//! `flush` writes them back and reloads the view from DRAM — so a driver that skips one
//! hands the device stale memory or reads stale memory itself, and the data shows it.
//! Dirty bytes are the ones that differ from what the view last synced with DRAM.
//! OWNERS: @runtime @drivers

use nexus_abi::DmaRun;
use nexus_driverkit::{CacheOps, DmaMemory};

use super::Shared;

/// A cache instruction, as the model saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheOp {
    /// Write back `(region, bytes)`.
    Clean(usize, usize),
    /// Write back and reload `(region, bytes)`.
    Flush(usize, usize),
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
    pub ops: Vec<CacheOp>,
}

impl Dram {
    fn locate(&self, bus: u64) -> Option<(usize, usize)> {
        for (id, region) in self.regions.iter().enumerate() {
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

    /// The device writes `data` at `bus`.
    pub fn write(&mut self, bus: u64, data: &[u8]) -> Option<()> {
        let (id, at) = self.locate(bus)?;
        self.regions[id].dram.get_mut(at..at + data.len())?.copy_from_slice(data);
        Some(())
    }

    fn by_cpu(&mut self, bytes: &[u8]) -> usize {
        let at = bytes.as_ptr() as usize;
        self.regions.iter().position(|r| r.cpu == at).expect("cache op on memory the model made")
    }
}

/// DMA memory for the device: a CPU view and the bus pieces behind it.
pub struct ModelMem {
    cpu: Vec<u8>,
    runs: Vec<DmaRun>,
}

impl ModelMem {
    /// `pieces` bus runs of whole pages, `gap` pages apart, from `bus` on (CPU-contiguous).
    pub fn new(m: &Shared, len: usize, bus: u64, pieces: usize, gap: u64) -> Self {
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
        m.borrow_mut().mem.regions.push(Region {
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

/// The cache instructions, acting on the model's DRAM.
pub struct ModelCache(pub Shared);

impl ModelCache {
    fn write_back(dram: &mut Dram, id: usize, bytes: &[u8]) {
        let region = &mut dram.regions[id];
        for (i, &b) in bytes.iter().enumerate() {
            if b != region.synced[i] {
                region.dram[i] = b;
                region.synced[i] = b;
            }
        }
    }
}

impl CacheOps for ModelCache {
    fn clean(&self, bytes: &[u8], block: usize) {
        assert_eq!(block, 64, "the harts' block");
        let mut m = self.0.borrow_mut();
        let id = m.mem.by_cpu(bytes);
        Self::write_back(&mut m.mem, id, bytes);
        m.mem.ops.push(CacheOp::Clean(id, bytes.len()));
    }

    fn flush(&self, bytes: &mut [u8], block: usize) {
        assert_eq!(block, 64, "the harts' block");
        let mut m = self.0.borrow_mut();
        let id = m.mem.by_cpu(bytes);
        Self::write_back(&mut m.mem, id, bytes);
        let region = &mut m.mem.regions[id];
        bytes.copy_from_slice(&region.dram);
        region.synced.copy_from_slice(&region.dram);
        m.mem.ops.push(CacheOp::Flush(id, bytes.len()));
    }
}
