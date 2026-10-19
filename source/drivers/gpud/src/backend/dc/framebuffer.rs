// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The scanout memory on the board: blocks made for the display controller — ONE contiguous run
//! each (it reads a base and a stride, never a list), so they lie inside the controller's reach
//! and the kernel names their bus address (RFC-0098 C4). Two kinds: the boot splash (one plane
//! of the mode, rows packed) the controller scans until the reveal, and windowd's framebuffer —
//! the whole shared layout (`nexus_display_proto::layout`: the planes and the atlas windowd
//! composes with), whose display plane the controller scans from the reveal on. The controller
//! does not snoop the caches: what the CPU wrote is cleaned out before the controller reads it
//! (Zicbom, the block size the controller's capability carries) — the splash, the whole plane
//! before a switch; and what the GPU drew is dropped before the CPU reads it (a readback).

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
}

impl Framebuffer {
    /// windowd's framebuffer: the whole layout; the controller scans its display plane.
    pub(super) fn layout() -> Result<Self, FramebufferError> {
        let start = layout::row_offset_bytes(layout::DISPLAY_ROW);
        Self::make(layout::RESOURCE_BYTES, start, layout::STRIDE_BYTES)
    }

    /// The boot splash: one `width`x`height` plane, rows packed.
    pub(super) fn splash(width: u16, height: u16) -> Result<Self, FramebufferError> {
        let stride = u32::from(width) * layout::BYTES_PER_PIXEL;
        Self::make(stride as usize * usize::from(height), 0, stride)
    }

    /// Made for the controller (zeroed by the kernel).
    fn make(len: usize, plane_start: usize, stride: u32) -> Result<Self, FramebufferError> {
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
        Ok(Framebuffer { mem, coherence, plane_start, stride })
    }

    /// The block's VMO (what the grant clones for windowd).
    pub(super) fn handle(&self) -> u32 {
        self.mem.handle()
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
        Plane {
            bus_addr: self.mem.runs()[0].bus + layout::row_offset_bytes(row) as u64,
            stride: self.stride,
            width,
            height,
        }
    }

    /// The bus address of the plane at layout row `row` (RFC-0101 §8c: the GPU's target).
    pub(super) fn plane_bus(&self, row: u32) -> u64 {
        self.mem.runs()[0].bus + layout::row_offset_bytes(row) as u64
    }

    /// The block's bus address: the layout's first byte, as the GPU adopts it (5c-3 P4b).
    pub(super) fn bus(&self) -> u64 {
        self.mem.runs()[0].bus
    }

    /// The whole block as the CPU sees it: the cache maintenance around a GPU frame.
    pub(super) fn bytes(&self) -> &[u8] {
        self.mem.bytes()
    }

    /// The plane at layout row `row` written back and dropped from the caches: before the GPU
    /// draws a plane the CPU wrote, before the CPU reads one the GPU drew.
    pub(super) fn flush_plane(&self, row: u32) {
        let DmaCoherence::Maintained { block } = self.coherence else {
            return;
        };
        let start = layout::row_offset_bytes(row);
        if let Some(plane) = self.mem.bytes().get(start..start + layout::PLANE_BYTES) {
            nexus_abi::cache_flush(plane, block);
        }
    }

    /// The bytes of one plane at layout row `row`, for the cache maintenance and the readback
    /// around a GPU job.
    pub(super) fn plane_bytes_mut(&mut self, row: u32) -> Option<&mut [u8]> {
        let start = layout::row_offset_bytes(row);
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
        let Some(bytes) = self.mem.bytes().get(layout::row_offset_bytes(row)..) else {
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

    /// `OP_READBACK` (RFC-0095): `req` of the `w`x`h` display plane into the caller's VMO, rows
    /// tight; with `READBACK_FREEZE` the display plane also becomes the retained plane (the base
    /// layer). Only the layout block has a retained plane — the splash refuses.
    pub(super) fn readback(
        &mut self,
        dest: u32,
        req: &nexus_display_proto::readback::Readback,
        w: u32,
        h: u32,
        front_row: u32,
    ) -> u8 {
        use crate::readback::{copy_plane, copy_rect_out, with_destination};
        use nexus_display_proto::{STATUS_DEVICE_ERROR, STATUS_MALFORMED, STATUS_OK};
        let display = layout::row_offset_bytes(front_row);
        if self.plane_start != layout::row_offset_bytes(layout::DISPLAY_ROW) {
            return STATUS_DEVICE_ERROR;
        }
        let stride = self.stride as usize;
        let copied = {
            let Some(plane) = self.mem.bytes().get(display..) else {
                return STATUS_DEVICE_ERROR;
            };
            with_destination(dest, req.bytes(), |dst| copy_rect_out(plane, stride, w, h, req, dst))
        };
        if !matches!(copied, Some(Ok(()))) {
            return STATUS_MALFORMED;
        }
        if req.flags & nexus_display_proto::readback::READBACK_FREEZE != 0 {
            let retained = layout::row_offset_bytes(layout::RETAINED_ROW);
            let bytes = self.mem.bytes_mut();
            if copy_plane(bytes, display, retained, stride, w as usize * 4, h as usize).is_err() {
                return STATUS_DEVICE_ERROR;
            }
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
