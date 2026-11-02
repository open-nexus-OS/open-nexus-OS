// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The device's buffers (RFC-0101 §3, TASK-0215 5c-3 P2a): a buffer is the device's own pages —
//! CPU-mapped (`CpuCached`, a DMA VMO: one region of gpud's address space) or the GPU's alone
//! (`GpuPrivate`, no region) — or a plane of the ring adopted by its bus address (the display's
//! memory). A buffer belongs to its client, is bound in ranges (`Mapping`) in any number of its
//! client's contexts, and is destroyed by its client once nothing binds it. The table is
//! reserved once and grows with the buffers (RFC-0114 §7: a window's surface is a buffer, and the
//! windows are as many as memory allows); a destroyed buffer's slot serves the next.

use alloc::vec::Vec;

use nexus_abi::DmaVmo;
use nexus_driverkit::gpu::Access;
use nexus_gfx::backend::gpu::mmu::{PageFlags, PAGE};
use nexus_gfx::backend::v2::{BoId, ClientId, DeviceError};
use nexus_service_topology::DEVICE_MMIO_SLOT;

use super::gpu_vmo::GpuVmo;

/// Buffers the table holds before it first grows: the device's own (firmware, contexts, the render
/// bundle, the desktop) and the first few dozen windows' surfaces.
const BOS_RESERVED: usize = 64;

// A buffer is a slot of the table, allocated once: boxing the DMA VMO's run list would make
// gpud allocate for every buffer a client creates.
#[allow(clippy::large_enum_variant)]
pub(super) enum Backing {
    /// The device's own pages, CPU-mapped (`CpuCached`).
    Vmo(DmaVmo),
    /// The device's own pages, the GPU's alone (`GpuPrivate`).
    Gpu(GpuVmo),
    /// A plane of the ring: the display's memory at a bus address, maintained by the display.
    Plane { bus: u64 },
    /// The display's surface object (ADR-0078): a lazy object made for the GPU, its pages named
    /// — and so committed — a chunk at a time as frames first sample them. The display owns it.
    Lazy { handle: u32 },
}

pub(super) struct Bo {
    pub client: ClientId,
    /// Bytes, a multiple of the page.
    pub len: usize,
    pub backing: Backing,
    /// Live bindings, in every context.
    pub bindings: u32,
    /// The open driver's flags for the buffer, the mapping policy since 5c-1 (TASK-0215): the
    /// GPU may cache it in its system-level cache; the parameter manager and the firmware own
    /// its pages.
    pub device_cached: bool,
    pub pm_fw_protect: bool,
}

impl Bo {
    /// The page flags of a binding, as the open driver maps the buffer (5c-1): only the buffers
    /// it creates with the bypass flag — the dataset, the free lists, the context state — stay
    /// out of the GPU's system-level cache; the parameter manager's and the firmware's own pages
    /// are protected.
    pub(super) fn page_flags(&self, access: Access) -> PageFlags {
        PageFlags {
            read_only: access == Access::ReadOnly,
            slc_bypass: !self.device_cached,
            pm_fw_protect: self.pm_fw_protect,
        }
    }

    /// Calls `page` with the bus address of every page of `offset..offset + len`, in order —
    /// walked, never collected (a list per binding, 16 KiB for a plane, ran gpud's heap dry:
    /// 5c-1's hang); `false` when a page is missing or `page` stopped the walk.
    pub(super) fn walk(&self, offset: u64, len: u64, mut page: impl FnMut(u64) -> bool) -> bool {
        let Some(end) = offset.checked_add(len) else { return false };
        if end > self.len as u64 || offset % PAGE != 0 || len % PAGE != 0 {
            return false;
        }
        match &self.backing {
            Backing::Vmo(v) => {
                let mut at = 0u64;
                let mut left = len / PAGE;
                for run in v.runs() {
                    let mut p = 0;
                    while p < run.len.div_ceil(PAGE) && left > 0 {
                        if at >= offset {
                            if !page(run.bus + p * PAGE) {
                                return false;
                            }
                            left -= 1;
                        }
                        at += PAGE;
                        p += 1;
                    }
                }
                left == 0
            }
            Backing::Gpu(g) => g.walk(DEVICE_MMIO_SLOT, offset as usize, len as usize, page),
            Backing::Plane { bus } => (0..len / PAGE).all(|i| page(bus + offset + i * PAGE)),
            Backing::Lazy { handle } => super::gpu_vmo::walk_object(
                (*handle, self.len),
                DEVICE_MMIO_SLOT,
                (offset as usize, len as usize),
                super::gpu_vmo::LAZY_CHUNK,
                page,
            ),
        }
    }

    /// The CPU view of `offset..offset + len`, for a CPU-mapped buffer.
    pub(super) fn bytes_mut(&mut self, offset: usize, len: usize) -> Option<&mut [u8]> {
        match &mut self.backing {
            Backing::Vmo(v) => v.bytes_mut().get_mut(offset..offset.checked_add(len)?),
            Backing::Gpu(_) | Backing::Plane { .. } | Backing::Lazy { .. } => None,
        }
    }

    pub(super) fn bytes(&self, offset: usize, len: usize) -> Option<&[u8]> {
        match &self.backing {
            Backing::Vmo(v) => v.bytes().get(offset..offset.checked_add(len)?),
            Backing::Gpu(_) | Backing::Plane { .. } | Backing::Lazy { .. } => None,
        }
    }
}

/// The device's buffer table.
pub(super) struct Bos {
    table: Vec<Option<Bo>>,
}

impl Bos {
    pub(super) fn new() -> Self {
        // Reserved once for the common case, so a usual session never moves the table; past it
        // the table grows — a growth memory refuses is the device's `Exhausted`, never a panic.
        Self { table: Vec::with_capacity(BOS_RESERVED) }
    }

    pub(super) fn insert(&mut self, bo: Bo) -> Result<BoId, DeviceError> {
        if let Some(i) = self.table.iter().position(Option::is_none) {
            self.table[i] = Some(bo);
            return Ok(BoId(i as u32));
        }
        let id = u32::try_from(self.table.len()).map_err(|_| DeviceError::Exhausted)?;
        self.table.try_reserve(1).map_err(|_| DeviceError::Exhausted)?;
        self.table.push(Some(bo));
        Ok(BoId(id))
    }

    /// `client`'s buffer `bo`.
    pub(super) fn get(&self, client: ClientId, bo: BoId) -> Result<&Bo, DeviceError> {
        let b = self.any(bo).ok_or(DeviceError::NoSuchBuffer)?;
        if b.client != client {
            return Err(DeviceError::NotOwner);
        }
        Ok(b)
    }

    pub(super) fn get_mut(&mut self, client: ClientId, bo: BoId) -> Result<&mut Bo, DeviceError> {
        let b = self
            .table
            .get_mut(bo.0 as usize)
            .and_then(Option::as_mut)
            .ok_or(DeviceError::NoSuchBuffer)?;
        if b.client != client {
            return Err(DeviceError::NotOwner);
        }
        Ok(b)
    }

    /// The buffer a binding names (its owner checked when it was bound).
    pub(super) fn any(&self, bo: BoId) -> Option<&Bo> {
        self.table.get(bo.0 as usize).and_then(Option::as_ref)
    }

    pub(super) fn any_mut(&mut self, bo: BoId) -> Option<&mut Bo> {
        self.table.get_mut(bo.0 as usize).and_then(Option::as_mut)
    }

    /// Destroys `client`'s buffer once nothing binds it; its pages go back with it.
    pub(super) fn remove(&mut self, client: ClientId, bo: BoId) -> Result<(), DeviceError> {
        if self.get(client, bo)?.bindings > 0 {
            return Err(DeviceError::BufferBound);
        }
        self.table[bo.0 as usize] = None;
        Ok(())
    }

    /// Buffers alive.
    pub(super) fn live(&self) -> usize {
        self.table.iter().flatten().count()
    }

    /// Bytes `client`'s buffers span together (TASK-0333 6e-4: charged to the client).
    pub(super) fn held_by(&self, client: ClientId) -> u64 {
        self.table
            .iter()
            .flatten()
            .filter(|b| b.client == client)
            .map(|b| b.len as u64)
            .fold(0, u64::saturating_add)
    }
}
