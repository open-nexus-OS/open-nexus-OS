// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The scanout memory on the board: ONE contiguous block made for the display controller (it
//! reads a base and a stride, never a list), so it lies inside the controller's reach and the
//! kernel names its bus address (RFC-0098 C4). It holds the whole shared layout
//! (`nexus_display_proto::layout`: the planes and the atlas windowd composes with); the
//! controller scans the display plane's rows. The controller does not snoop the caches: what the
//! CPU wrote is cleaned out before the controller reads it (Zicbom, the block size the
//! controller's capability carries).

use nexus_abi::{DmaCoherence, DmaVmo};
use nexus_display_proto::layout;
use nexus_gfx::backend::dc::Plane;
use nexus_service_topology::slots::gpud as topo;

/// Why the framebuffer could not be made.
#[derive(Clone, Copy, Debug)]
pub(super) enum FramebufferError {
    /// The controller does not snoop and the harts cannot clean for it.
    Unmaintainable,
    /// The kernel made no contiguous block of the layout's size for the controller.
    Make,
    /// The block came back as more than one run (the controller takes one base).
    Runs(usize),
}

impl core::fmt::Display for FramebufferError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FramebufferError::Unmaintainable => {
                f.write_str("the controller cannot be kept coherent")
            }
            FramebufferError::Make => f.write_str("no contiguous block for the controller"),
            FramebufferError::Runs(n) => write!(f, "{n} runs, the controller takes one base"),
        }
    }
}

pub(super) struct Framebuffer {
    mem: DmaVmo,
    coherence: DmaCoherence,
}

const DISPLAY_START: usize = layout::row_offset_bytes(layout::DISPLAY_ROW);

impl Framebuffer {
    /// Make the framebuffer for the controller (zeroed by the kernel).
    pub(super) fn make() -> Result<Self, FramebufferError> {
        let device = topo::DISPLAY_CONTROLLER;
        let coherence =
            nexus_abi::device_dma_coherence(device).map_err(|_| FramebufferError::Make)?;
        if matches!(coherence, DmaCoherence::Unmaintainable) {
            return Err(FramebufferError::Unmaintainable);
        }
        let mem = DmaVmo::contiguous(device, layout::RESOURCE_BYTES)
            .map_err(|_| FramebufferError::Make)?;
        if mem.runs().len() != 1 {
            return Err(FramebufferError::Runs(mem.runs().len()));
        }
        Ok(Framebuffer { mem, coherence })
    }

    /// The bus address the controller reads the framebuffer's first byte at.
    pub(super) fn bus(&self) -> u64 {
        self.mem.runs()[0].bus
    }

    /// The display plane the controller scans: `width`x`height` pixels at the layout's stride.
    pub(super) fn display_plane(&self, width: u16, height: u16) -> Plane {
        Plane {
            bus_addr: self.bus() + DISPLAY_START as u64,
            stride: layout::STRIDE_BYTES,
            width,
            height,
        }
    }

    /// The display plane's bytes, for the CPU to draw into.
    pub(super) fn display_plane_mut(&mut self) -> &mut [u8] {
        &mut self.mem.bytes_mut()[DISPLAY_START..DISPLAY_START + layout::PLANE_BYTES]
    }

    /// Make what the CPU wrote to the display plane visible to the controller.
    pub(super) fn clean_display_plane(&self) {
        if let DmaCoherence::Maintained { block } = self.coherence {
            let plane = &self.mem.bytes()[DISPLAY_START..DISPLAY_START + layout::PLANE_BYTES];
            nexus_abi::cache_clean(plane, block);
        }
    }
}
