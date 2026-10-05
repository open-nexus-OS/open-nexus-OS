// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The controller's DMA memory (RFC-0099 §3): every structure lives in a contiguous region
//! made for the controller's device and shared with it for its lifetime
//! (`nexus_driverkit::DmaShared`). A region of at most 64 KiB is one block aligned to its own
//! size (the kernel's buddy invariant), so no ring segment inside it crosses a 64 KiB boundary
//! and every offset that is a multiple of 64 is 64-byte aligned on the bus — [`Region::new`]
//! checks it from the runs the kernel reports instead of trusting it.

use nexus_driverkit::{CacheOps, DmaMemory, DmaShared};

/// The largest region (a ring segment must not cross 64 KiB).
pub const REGION_MAX: usize = 64 * 1024;

/// Makes the controller's memory.
pub trait DmaAlloc {
    /// The memory.
    type Mem: DmaMemory;
    /// The cache instructions.
    type Cache: CacheOps;

    /// Zeroed, physically contiguous memory of `len` bytes (a power of two of at most
    /// [`REGION_MAX`]) the controller can reach.
    fn shared(&mut self, len: usize) -> Option<DmaShared<Self::Mem, Self::Cache>>;
}

/// A region and its bus address.
pub struct Region<M: DmaMemory, C: CacheOps> {
    /// The memory.
    pub mem: DmaShared<M, C>,
    bus: u64,
}

impl<M: DmaMemory, C: CacheOps> Region<M, C> {
    /// `mem` as a region: one run, aligned to its size; `None` otherwise. Fresh memory the
    /// kernel zeroed with CPU stores may sit dirty in the harts' caches: it is written back and
    /// dropped once, so no eviction later overwrites what the controller wrote.
    pub fn new(mut mem: DmaShared<M, C>) -> Option<Self> {
        let len = mem.len();
        let [run] = mem.runs() else { return None };
        let bus = run.bus;
        let aligned = len.is_power_of_two() && len <= REGION_MAX && bus % len as u64 == 0;
        if !aligned || (run.len as usize) < len {
            return None;
        }
        mem.observe(0..len);
        Some(Self { bus, mem })
    }

    /// The bus address of `offset`.
    #[must_use]
    pub fn bus(&self, offset: usize) -> u64 {
        self.bus + offset as u64
    }

    /// The offset of bus address `bus`, if it lies in the region.
    #[must_use]
    pub fn offset_of(&self, bus: u64) -> Option<usize> {
        let offset = bus.checked_sub(self.bus)?;
        (offset < self.mem.len() as u64).then_some(offset as usize)
    }

    /// Write `data` at `offset` and publish it.
    pub fn put(&mut self, offset: usize, data: &[u8]) {
        if let Some(dst) = self.mem.bytes_mut().get_mut(offset..offset + data.len()) {
            dst.copy_from_slice(data);
            self.mem.publish(offset..offset + data.len());
        }
    }
}
