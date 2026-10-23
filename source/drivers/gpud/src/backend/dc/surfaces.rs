// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board display's surface object (ADR-0078, TASK-0333 6b-2b): the wallpaper, the retained
//! plane and the surface atlas — every row of `nexus_display_proto::layout` but the ring's — as
//! ONE lazy object made for the GPU, sized from the machine's RAM (`crate::gpu_share`) in whole
//! display planes. windowd is granted it and writes its rows (`vmo_write` commits what it
//! writes); the GPU is told its pages a band at a time as frames first sample them (naming
//! commits); the ring's rows in it are a hole nothing touches. gpud maps it to keep the caches
//! coherent for the GPU (it cleans the rows windowd wrote before a pass reads them) and to copy a
//! frozen frame into the retained plane — a page it touches is committed then.
//! OWNERS: @gpu @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the share and the page arithmetic host-tested (`crate::gpu_share`,
//!   `crate::surface_pages`); the board (`gpud: layout split ok (`)

use nexus_abi::{DmaCoherence, LazyDmaVmo};
use nexus_display_proto::layout;
use nexus_service_topology::slots::gpud as topo;
use nexus_service_topology::DEVICE_MMIO_SLOT;

use super::framebuffer::FramebufferError;

pub(super) struct Surfaces {
    mem: LazyDmaVmo,
    /// The cache block the GPU's reads are maintained in; 0 when coherent.
    block: usize,
}

impl Surfaces {
    /// `planes` display planes for the device that reads them — the GPU; the controller's reach
    /// on a board granted no GPU (nothing draws from them there, the grant still stands) —
    /// nothing committed.
    pub(super) fn make(planes: u32) -> Result<Self, FramebufferError> {
        let device =
            if crate::gpu::granted() { DEVICE_MMIO_SLOT } else { topo::DISPLAY_CONTROLLER };
        let coherence =
            nexus_abi::device_dma_coherence(device).map_err(|_| FramebufferError::Make)?;
        let block = match coherence {
            DmaCoherence::Unmaintainable => return Err(FramebufferError::Unmaintainable),
            DmaCoherence::Maintained { block } => block,
            _ => 0,
        };
        let len = planes as usize * layout::PLANE_BYTES;
        let mem = LazyDmaVmo::new(device, len).map_err(|_| FramebufferError::Make)?;
        Ok(Self { mem, block })
    }

    /// The object (what the grant clones for windowd, what the GPU's binding names).
    pub(super) fn handle(&self) -> u32 {
        self.mem.handle()
    }

    /// Bytes reserved: whole display planes.
    pub(super) fn len(&self) -> usize {
        self.mem.len()
    }

    /// The CPU's view by layout row — touched only where the CPU wrote or reads.
    pub(super) fn bytes(&self) -> &[u8] {
        self.mem.bytes()
    }

    /// The cache block the GPU's reads are maintained in; 0 when coherent.
    pub(super) fn cache_block(&self) -> usize {
        self.block
    }

    /// The freeze (RFC-0095): the `w`x`h` frame on screen, `plane` (a slot of the ring), copied
    /// into the retained plane — the base the GPU's scene starts from until the thaw.
    pub(super) fn freeze(&mut self, plane: &[u8], w: u32, h: u32) -> bool {
        let at = layout::row_offset_bytes(layout::RETAINED_ROW);
        let Some(retained) = self.mem.bytes_mut().get_mut(at..at + layout::PLANE_BYTES) else {
            return false;
        };
        let stride = layout::STRIDE_BYTES as usize;
        crate::readback::copy_rows(plane, retained, stride, w as usize * 4, h as usize).is_ok()
    }
}
