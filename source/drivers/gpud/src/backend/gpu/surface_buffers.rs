// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The app surfaces in windowd's context on the board (RFC-0114 §1, ADR-0092): each object gpud
//! made for an app — a lazy object for the GPU — adopted as a buffer of windowd's and bound
//! read-only WHOLE at allocation: every page named (and so committed), the tables written, the
//! MMU's caches flushed once. No frame ever enters a page or waits on a flush for it, and nothing
//! of it is cleaned here: the app cleans what it drew (`vmo_clean`) before it presents, the kernel
//! cleaned the zeroes. Its addresses come from the tail of the context's private GiB — the GiB
//! the scratches already open, so no directory more — a first-fit range per object on 2 MiB
//! boundaries (a table of its own each). A frame samples `(source, slot, row, x)` at the object's
//! address plus `(slot × rows + row) × stride + x × 4` (`descriptor`).
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the address ranges host-tested (`crate::surface_va`); the board's frames
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use alloc::vec::Vec;

use nexus_driverkit::gpu::{Access, GpuVa};
use nexus_gfx::backend::v2::{BoId, ClientId, ContextId, DeviceError, GfxDevice, Mapping};

use super::device_v2::Device;
use crate::surface_va::{surface_va, VA_ALIGN};
use crate::surfaces::Geometry;

/// One object bound in windowd's context.
#[derive(Clone, Copy, Debug)]
struct Bound {
    buffer: u32,
    bo: BoId,
    va: u64,
    /// Bytes the range spans (the object rounded up to the alignment).
    span: u64,
    geometry: Geometry,
}

/// Every app surface bound in windowd's context.
pub(crate) struct SurfaceBuffers {
    bound: Vec<Bound>,
}

impl SurfaceBuffers {
    pub(crate) const fn new() -> Self {
        Self { bound: Vec::new() }
    }

    /// Bytes every bound object spans (charged to its app, not to windowd's context).
    pub(crate) fn bytes(&self) -> u64 {
        self.bound.iter().map(|b| b.geometry.len() as u64).fold(0, u64::saturating_add)
    }

    /// Object `vmo` — source `buffer` of `geometry` — adopted as `who`'s buffer and bound
    /// read-only whole. False: refused (the ceiling, the tables, no range); nothing kept.
    pub(crate) fn bind(
        &mut self,
        dev: &mut Device,
        who: (ClientId, ContextId),
        (buffer, vmo): (u32, u32),
        geometry: &Geometry,
    ) -> bool {
        if self.bound.try_reserve(1).is_err() || self.bound.iter().any(|b| b.buffer == buffer) {
            return false;
        }
        let len = geometry.len() as u64;
        let taken: Vec<(u64, u64)> = self.bound.iter().map(|b| (b.va, b.span)).collect();
        let Some(va) = surface_va(&taken, len) else { return false };
        let Ok(bo) = dev.adopt_lazy(who.0, vmo, len as usize) else { return false };
        let m = Mapping { bo, offset: 0, len, va: GpuVa(va), access: Access::ReadOnly };
        if dev.vm_bind(who.0, who.1, m).is_err() {
            let _ = dev.destroy_bo(who.0, bo);
            return false;
        }
        let span = len.next_multiple_of(VA_ALIGN);
        self.bound.push(Bound { buffer, bo, va, span, geometry: *geometry });
        true
    }

    /// Source `buffer` unbound and its buffer given back (its last frame retired).
    pub(crate) fn unbind(
        &mut self,
        dev: &mut Device,
        who: (ClientId, ContextId),
        buffer: u32,
    ) -> Result<(), DeviceError> {
        let Some(i) = self.bound.iter().position(|b| b.buffer == buffer) else { return Ok(()) };
        let b = self.bound.swap_remove(i);
        dev.vm_unbind(who.0, who.1, GpuVa(b.va))?;
        dev.destroy_bo(who.0, b.bo)
    }

    /// Every object given back with windowd's context (its bindings went with it).
    pub(crate) fn release(&mut self, dev: &mut Device, client: ClientId) {
        for b in self.bound.drain(..) {
            let _ = dev.destroy_bo(client, b.bo);
        }
    }

    /// The address and stride (texels) of `w`×`h` at column `x`, row `row` of slot `slot` of
    /// source `buffer` — `None` past the object (gpud checked every layer at intake).
    pub(crate) fn texels(
        &self,
        buffer: u32,
        slot: u8,
        (x, row, w, h): (u32, u32, u32, u32),
    ) -> Option<(u64, u32)> {
        let b = self.bound.iter().find(|b| b.buffer == buffer)?;
        let g = &b.geometry;
        if !g.admits(slot, x, row, w, h) {
            return None;
        }
        let at =
            g.slot_offset(slot) as u64 + u64::from(row) * u64::from(g.stride) + u64::from(x) * 4;
        Some((b.va + at, g.stride / 4))
    }
}
