// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: How the virtio GPU reads the app surfaces gpud made (RFC-0114 §1, ADR-0092) — eager
//! objects made for the virtio device. On the 2D path the CPU executor blends a buffer layer
//! from the object's read-only mapping, where the app drew it (`source`); on virgl the object is
//! a 3D texture resource over its runs (`virgl_surfaces`), a layer's rows transferred to the
//! host when its content changed. Kept where the executor keeps what a present reads besides
//! the framebuffer (`cpu_frame::CpuFrame`), made at allocation, gone when the object retires.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the CPU blend host-tested (`nexus_gfx` `raster_tests`); the QEMU display lanes
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use alloc::vec::Vec;

use crate::surfaces::Geometry;

/// One object as the virtio GPU reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Backed {
    pub(crate) buffer: u32,
    pub(crate) geometry: Geometry,
    /// The 2D path: gpud's read-only mapping of the object.
    #[cfg(not(feature = "virgl"))]
    va: usize,
    #[cfg(not(feature = "virgl"))]
    len: usize,
    /// virgl: the texture resource over the object and its sampler view.
    #[cfg(feature = "virgl")]
    pub(crate) resource: u32,
    #[cfg(feature = "virgl")]
    pub(crate) view: u32,
}

/// Every app surface the virtio GPU reads.
pub(crate) struct SurfaceBacking {
    backed: Vec<Backed>,
}

impl SurfaceBacking {
    pub(crate) const fn new() -> Self {
        Self { backed: Vec::new() }
    }

    /// The object of source `buffer`.
    pub(crate) fn get(&self, buffer: u32) -> Option<&Backed> {
        self.backed.iter().find(|b| b.buffer == buffer)
    }

    /// Keeps `b`; false when the table cannot grow.
    pub(crate) fn keep(&mut self, b: Backed) -> bool {
        if self.backed.try_reserve(1).is_err() {
            return false;
        }
        self.backed.push(b);
        true
    }

    /// Takes the object of source `buffer` out.
    pub(crate) fn take(&mut self, buffer: u32) -> Option<Backed> {
        let i = self.backed.iter().position(|b| b.buffer == buffer)?;
        Some(self.backed.swap_remove(i))
    }
}

#[cfg(all(feature = "os-lite", target_os = "none", not(feature = "virgl")))]
impl SurfaceBacking {
    /// The 2D path: object `vmo` mapped read-only for the CPU executor.
    pub(crate) fn map(&mut self, buffer: u32, vmo: u32, geometry: &Geometry) -> bool {
        use nexus_abi::page_flags::{READ, USER, VALID};
        let len = geometry.len();
        let Ok(va) = nexus_abi::vm_map(vmo, 0, len, VALID | USER | READ) else { return false };
        let kept = self.keep(Backed { buffer, geometry: *geometry, va, len });
        if !kept {
            let _ = nexus_abi::vm_unmap(va, len);
        }
        kept
    }

    /// The 2D path: the mapping of source `buffer` goes.
    pub(crate) fn unmap(&mut self, buffer: u32) {
        if let Some(b) = self.take(buffer) {
            let _ = nexus_abi::vm_unmap(b.va, b.len);
        }
    }

    /// Slot `slot` of source `buffer` as the executor blends from it: its bytes, row by row.
    pub(crate) fn source(
        &self,
        buffer: u32,
        slot: u8,
    ) -> Option<nexus_gfx::raster::BlendSource<'_>> {
        let b = self.get(buffer)?;
        let g = &b.geometry;
        if slot >= g.slots {
            return None;
        }
        let at = g.slot_offset(slot);
        let bytes = usize::from(g.rows) * g.stride as usize;
        // SAFETY: `va..va + len` is this service's own read-only mapping of the object, alive
        // until `unmap`; the slot lies inside it (`Geometry::len` holds every slot); the app
        // writes it concurrently — what a frame reads is the app's to keep consistent (it presents
        // a slot only once drawn, RFC-0114 §5), never memory outside the object.
        let all = unsafe { core::slice::from_raw_parts(b.va as *const u8, b.len) };
        Some(nexus_gfx::raster::BlendSource {
            bytes: all.get(at..at + bytes)?,
            stride: g.stride as usize,
        })
    }
}
