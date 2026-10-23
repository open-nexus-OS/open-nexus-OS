// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The scanout memory on the board: blocks made for the display controller — ONE contiguous run
//! each (it reads a base and a stride, never a list), so they lie inside the controller's reach
//! and the kernel names their bus address (RFC-0098 C4). Two kinds: the boot splash (one plane
//! of the mode, rows packed) the controller scans until the reveal, and the scan-out ring — the
//! three slots of `nexus_display_proto::layout::RING_ROWS` the GPU draws windowd's frames into
//! and the controller flips among from the reveal on, addressed by their layout rows. The ring is
//! the only memory of the display committed up front; the wallpaper, the retained plane and the
//! atlas are the surface object's (`surfaces.rs`, ADR-0078). The controller does not snoop the
//! caches: what the CPU wrote is cleaned out before the controller reads it (Zicbom, the block
//! size the controller's capability carries) — the splash, the whole plane before a switch; and
//! what the GPU drew is dropped before the CPU reads it (a readback).

use nexus_abi::{DmaCoherence, DmaVmo};
use nexus_display_proto::layout;
use nexus_gfx::backend::dc::{damage_spans, Plane};
use nexus_gfx::backend::types::Rect;
use nexus_service_topology::slots::gpud as topo;

/// Why a block could not be made.
#[derive(Clone, Copy, Debug)]
pub(super) enum FramebufferError {
    /// The controller does not snoop and the harts cannot clean for it.
    Unmaintainable,
    /// The kernel made no contiguous block of that size for the controller.
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
    /// Where the scanned plane starts in the block, and its row stride.
    plane_start: usize,
    stride: u32,
    /// The layout row the block's first byte holds: the ring's first slot for the scan-out
    /// ring, 0 for the splash (a plane of its own).
    first_row: u32,
}

impl Framebuffer {
    /// The scan-out ring (ADR-0078): the three slots the controller flips among, one block from
    /// layout row `RING_ROWS[0]`.
    pub(super) fn scanout() -> Result<Self, FramebufferError> {
        let len = layout::RING_ROWS.len() * layout::PLANE_BYTES;
        Self::make(len, 0, layout::STRIDE_BYTES, layout::RING_ROWS[0])
    }

    /// The boot splash: one `width`x`height` plane, rows packed.
    pub(super) fn splash(width: u16, height: u16) -> Result<Self, FramebufferError> {
        let stride = u32::from(width) * layout::BYTES_PER_PIXEL;
        Self::make(stride as usize * usize::from(height), 0, stride, 0)
    }

    /// Bytes of the block.
    pub(super) fn len(&self) -> usize {
        self.mem.len()
    }

    /// Where layout row `row` lies in the block (the ring's rows; a row before the block's first
    /// is its first).
    fn at(&self, row: u32) -> usize {
        layout::row_offset_bytes(row.saturating_sub(self.first_row))
    }

    /// Made for the controller (zeroed by the kernel).
    fn make(
        len: usize,
        plane_start: usize,
        stride: u32,
        first_row: u32,
    ) -> Result<Self, FramebufferError> {
        let device = topo::DISPLAY_CONTROLLER;
        let coherence =
            nexus_abi::device_dma_coherence(device).map_err(|_| FramebufferError::Make)?;
        if matches!(coherence, DmaCoherence::Unmaintainable) {
            return Err(FramebufferError::Unmaintainable);
        }
        let mem = DmaVmo::contiguous(device, len).map_err(|_| FramebufferError::Make)?;
        if mem.runs().len() != 1 {
            return Err(FramebufferError::Runs(mem.runs().len()));
        }
        Ok(Framebuffer { mem, coherence, plane_start, stride, first_row })
    }

    /// The scanned plane at `width`x`height`: the bus address the controller reads its first
    /// byte at, and its stride.
    pub(super) fn plane(&self, width: u16, height: u16) -> Plane {
        Plane {
            bus_addr: self.mem.runs()[0].bus + self.plane_start as u64,
            stride: self.stride,
            width,
            height,
        }
    }

    /// The plane that starts at layout row `row` (a slot of the ring, RFC-0101 §8).
    pub(super) fn plane_at(&self, row: u32, width: u16, height: u16) -> Plane {
        Plane { bus_addr: self.plane_bus(row), stride: self.stride, width, height }
    }

    /// The bus address of the plane at layout row `row` (RFC-0101 §8c: the GPU's target).
    pub(super) fn plane_bus(&self, row: u32) -> u64 {
        self.mem.runs()[0].bus + self.at(row) as u64
    }

    /// The block's bus address: its first slot, as the GPU adopts the ring (5c-3 P4b).
    pub(super) fn bus(&self) -> u64 {
        self.mem.runs()[0].bus
    }

    /// The plane at layout row `row` written back and dropped from the caches: before the GPU
    /// draws a plane the CPU wrote, before the CPU reads one the GPU drew.
    pub(super) fn flush_plane(&self, row: u32) {
        let DmaCoherence::Maintained { block } = self.coherence else {
            return;
        };
        if let Some(plane) = self.plane_bytes(row) {
            nexus_abi::cache_flush(plane, block);
        }
    }

    /// The bytes of one plane at layout row `row`: the frame a freeze keeps.
    pub(super) fn plane_bytes(&self, row: u32) -> Option<&[u8]> {
        let start = self.at(row);
        self.mem.bytes().get(start..start + layout::PLANE_BYTES)
    }

    /// The bytes of one plane at layout row `row`, for the cache maintenance and the readback
    /// around a GPU job.
    pub(super) fn plane_bytes_mut(&mut self, row: u32) -> Option<&mut [u8]> {
        let start = self.at(row);
        self.mem.bytes_mut().get_mut(start..start + layout::PLANE_BYTES)
    }

    /// The cache block the controller's memory is maintained in; 0 when coherent.
    pub(super) fn cache_block(&self) -> usize {
        match self.coherence {
            DmaCoherence::Maintained { block } => block,
            _ => 0,
        }
    }

    /// Make `damage` of the plane at layout row `row` visible to the controller.
    pub(super) fn clean_at(&self, row: u32, plane: &Plane, damage: Rect) {
        let DmaCoherence::Maintained { block } = self.coherence else {
            return;
        };
        let Some(bytes) = self.mem.bytes().get(self.at(row)..) else {
            return;
        };
        for span in damage_spans(plane, damage.x, damage.y, damage.width, damage.height) {
            if let Some(run) = bytes.get(span) {
                nexus_abi::cache_clean(run, block);
            }
        }
    }

    /// The scanned plane's bytes, for the CPU to draw the splash into.
    pub(super) fn plane_mut(&mut self) -> &mut [u8] {
        &mut self.mem.bytes_mut()[self.plane_start..]
    }

    /// `OP_READBACK` (RFC-0095): `req` of the `w`x`h` plane on screen (the ring's slot at layout
    /// row `front_row`) into the caller's VMO, rows tight. Only the scan-out ring holds a frame
    /// to read — the splash refuses; a freeze's copy into the retained plane is the surface
    /// object's (`Surfaces::freeze`).
    pub(super) fn readback(
        &self,
        dest: u32,
        req: &nexus_display_proto::readback::Readback,
        w: u32,
        h: u32,
        front_row: u32,
    ) -> u8 {
        use crate::readback::{copy_rect_out, with_destination};
        use nexus_display_proto::{STATUS_DEVICE_ERROR, STATUS_MALFORMED, STATUS_OK};
        if self.first_row != layout::RING_ROWS[0] {
            return STATUS_DEVICE_ERROR;
        }
        let stride = self.stride as usize;
        let Some(plane) = self.plane_bytes(front_row) else {
            return STATUS_DEVICE_ERROR;
        };
        let copied =
            with_destination(dest, req.bytes(), |dst| copy_rect_out(plane, stride, w, h, req, dst));
        if !matches!(copied, Some(Ok(()))) {
            return STATUS_MALFORMED;
        }
        STATUS_OK
    }

    /// Make the `damage` of `plane` (screen coordinates, clipped to it) visible to the controller.
    pub(super) fn clean(&self, plane: &Plane, damage: Rect) {
        let DmaCoherence::Maintained { block } = self.coherence else {
            return;
        };
        let bytes = &self.mem.bytes()[self.plane_start..];
        for span in damage_spans(plane, damage.x, damage.y, damage.width, damage.height) {
            if let Some(run) = bytes.get(span) {
                nexus_abi::cache_clean(run, block);
            }
        }
    }
}
