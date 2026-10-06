// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The pointer as a layer of the display controller (TASK-0251 P2a step 3c): one 64×64 ARGB8888
//! block made for the controller (its bus address, like the planes), read through the pointer's
//! channel into the composer's pointer layer — the stock system's own arrangement
//! (`docs/board/measurements/2026-10-05-cursor-layer/`). A sprite windowd uploads or selects is
//! laid into the block and cleaned out of the caches; a move is the layer's rectangle. No
//! present carries the pointer on this path: the controller composites it at scan-out, as the
//! virtio path's cursor queue does.

use nexus_abi::{DmaCoherence, DmaVmo};
use nexus_gfx::backend::dc::{cursor_rect, lay_cursor, regs, CursorPlane, CursorRect};
use nexus_gfx::backend::error::GfxError;
use nexus_service_topology::slots::gpud as topo;

use super::framebuffer::FramebufferError;

pub(super) struct CursorLayer {
    mem: DmaVmo,
    coherence: DmaCoherence,
    /// The active sprite's hotspot (where the pointer is inside the buffer).
    hot: (u32, u32),
    /// The pointer's last position (screen coordinates), once windowd moved it.
    pos: Option<(i32, i32)>,
    /// The layer is on the composer.
    pub(super) armed: bool,
}

impl CursorLayer {
    /// The block for the controller (zeroed by the kernel: transparent).
    pub(super) fn make() -> Result<Self, FramebufferError> {
        let device = topo::DISPLAY_CONTROLLER;
        let coherence =
            nexus_abi::device_dma_coherence(device).map_err(|_| FramebufferError::Make)?;
        if matches!(coherence, DmaCoherence::Unmaintainable) {
            return Err(FramebufferError::Unmaintainable);
        }
        let mem =
            DmaVmo::contiguous(device, regs::CURSOR_BYTES).map_err(|_| FramebufferError::Make)?;
        if mem.runs().len() != 1 {
            return Err(FramebufferError::Runs(mem.runs().len()));
        }
        Ok(CursorLayer { mem, coherence, hot: (0, 0), pos: None, armed: false })
    }

    /// The block as the controller reads it.
    pub(super) fn plane(&self) -> CursorPlane {
        CursorPlane { bus_addr: self.mem.runs()[0].bus }
    }

    /// Lay a `w`×`h` premultiplied BGRA sprite with hotspot `hot` into the block, cleaned for
    /// the controller. Refused when the sprite does not fit the 64×64 block.
    pub(super) fn store(
        &mut self,
        bgra: &[u8],
        w: u32,
        h: u32,
        hot: (u32, u32),
    ) -> Result<(), GfxError> {
        lay_cursor(self.mem.bytes_mut(), bgra, w, h).ok_or(GfxError::InvalidArgument)?;
        if let DmaCoherence::Maintained { block } = self.coherence {
            nexus_abi::cache_clean(&self.mem.bytes()[..regs::CURSOR_BYTES], block);
        }
        self.hot = hot;
        Ok(())
    }

    /// Note the pointer's position; the layer's rectangle for it on a `width`×`height` mode
    /// (`None`: nothing of the sprite is on screen).
    pub(super) fn place(&mut self, x: i32, y: i32, width: u16, height: u16) -> Option<CursorRect> {
        self.pos = Some((x, y));
        self.rect(width, height)
    }

    /// The layer's rectangle at the last position (the mode's origin until windowd moved it).
    pub(super) fn rect(&self, width: u16, height: u16) -> Option<CursorRect> {
        let (x, y) = self.pos.unwrap_or((self.hot.0 as i32, self.hot.1 as i32));
        cursor_rect(x, y, self.hot, u32::from(width), u32::from(height))
    }
}
