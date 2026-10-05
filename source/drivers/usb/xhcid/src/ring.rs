// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Rings (xHCI 1.2 §4.9): a [`Producer`] ring the CPU fills and the controller consumes — the
//! command ring and every transfer ring — and the [`EventRing`] the controller fills and the
//! CPU consumes. Ownership of an entry is its cycle bit: the producer writes entries with its
//! cycle state and toggles the state where the segment's link TRB sends it back to the start.
//!
//! On a non-coherent machine a TRB becomes visible in two steps — its fields with the cycle
//! bit still showing "not yours", then the cycle bit — each written back before the next, so
//! the controller never reads a valid cycle bit with stale fields; the doorbell follows the
//! second write-back (`publish` fences). An event is observed (its line dropped) before it is
//! read, so the CPU never mistakes a stale line for the controller's write.

use nexus_driverkit::{CacheOps, DmaMemory};

use crate::memory::Region;
use crate::trb::{Trb, CHAIN, CYCLE};

/// Bytes per TRB.
pub const TRB_LEN: usize = 16;

/// A ring the CPU produces into: one segment of `len` TRBs at `offset` of a region, the last
/// a link back to the first.
#[derive(Clone, Copy, Debug)]
pub struct Producer {
    offset: usize,
    len: usize,
    enqueue: usize,
    cycle: bool,
}

impl Producer {
    /// A ring over zeroed memory (every TRB's cycle bit 0: nothing for the controller).
    #[must_use]
    pub const fn new(offset: usize, len: usize) -> Self {
        Self { offset, len, enqueue: 0, cycle: true }
    }

    /// The ring's start as the controller is programmed with it (bit 0 = the cycle state).
    #[must_use]
    pub fn start<M: DmaMemory, C: CacheOps>(&self, region: &Region<M, C>) -> u64 {
        region.bus(self.offset) | 1
    }

    /// Where the next TRB goes, with the cycle state in bit 0 (Set TR Dequeue Pointer).
    #[must_use]
    pub fn dequeue_pointer<M: DmaMemory, C: CacheOps>(&self, region: &Region<M, C>) -> u64 {
        region.bus(self.offset + self.enqueue * TRB_LEN) | u64::from(self.cycle)
    }

    /// TRBs free before the link.
    #[must_use]
    pub const fn room(&self) -> usize {
        self.len - 1 - self.enqueue
    }

    /// Append `trb` and return its bus address; at the segment's end the link carries the
    /// producer back to the start (chained when the TD continues past it).
    pub fn push<M: DmaMemory, C: CacheOps>(&mut self, region: &mut Region<M, C>, trb: Trb) -> u64 {
        let at = self.offset + self.enqueue * TRB_LEN;
        let address = region.bus(at);
        write_owned(region, at, trb, self.cycle);
        self.enqueue += 1;
        if self.enqueue == self.len - 1 {
            let link = self.offset + self.enqueue * TRB_LEN;
            let mut trb = Trb::link(region.bus(self.offset));
            if self.chained_last(region) {
                trb.control |= CHAIN;
            }
            write_owned(region, link, trb, self.cycle);
            self.enqueue = 0;
            self.cycle = !self.cycle;
        }
        address
    }

    fn chained_last<M: DmaMemory, C: CacheOps>(&self, region: &Region<M, C>) -> bool {
        let last = self.offset + (self.len - 2) * TRB_LEN;
        let bytes = region.mem.bytes().get(last..last + TRB_LEN).unwrap_or(&[]);
        Trb::from_bytes(bytes).control & CHAIN != 0
    }
}

/// Write `trb` at `at` so the controller sees its fields before its cycle bit.
fn write_owned<M: DmaMemory, C: CacheOps>(
    region: &mut Region<M, C>,
    at: usize,
    mut trb: Trb,
    cycle: bool,
) {
    trb.control = (trb.control & !CYCLE) | u32::from(!cycle);
    region.put(at, &trb.to_bytes());
    trb.control = (trb.control & !CYCLE) | u32::from(cycle);
    region.put(at + 12, &trb.control.to_le_bytes());
}

/// The event ring: one segment of `len` TRBs at `offset` of a region.
#[derive(Clone, Copy, Debug)]
pub struct EventRing {
    offset: usize,
    len: usize,
    dequeue: usize,
    cycle: bool,
}

impl EventRing {
    /// A ring over zeroed memory (the controller's first lap writes cycle 1).
    #[must_use]
    pub const fn new(offset: usize, len: usize) -> Self {
        Self { offset, len, dequeue: 0, cycle: true }
    }

    /// The segment's start and size, for the event ring segment table.
    #[must_use]
    pub fn segment<M: DmaMemory, C: CacheOps>(&self, region: &Region<M, C>) -> (u64, u32) {
        (region.bus(self.offset), self.len as u32)
    }

    /// The next event the controller wrote, if any.
    pub fn next<M: DmaMemory, C: CacheOps>(&mut self, region: &mut Region<M, C>) -> Option<Trb> {
        let at = self.offset + self.dequeue * TRB_LEN;
        region.mem.observe(at..at + TRB_LEN);
        let trb = Trb::from_bytes(region.mem.bytes().get(at..at + TRB_LEN)?);
        if trb.cycle() != self.cycle {
            return None;
        }
        self.dequeue += 1;
        if self.dequeue == self.len {
            self.dequeue = 0;
            self.cycle = !self.cycle;
        }
        Some(trb)
    }

    /// The dequeue pointer for ERDP: the next event to read.
    #[must_use]
    pub fn dequeue_pointer<M: DmaMemory, C: CacheOps>(&self, region: &Region<M, C>) -> u64 {
        region.bus(self.offset + self.dequeue * TRB_LEN)
    }
}
