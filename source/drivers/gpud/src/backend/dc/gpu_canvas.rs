// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A plane of the ring lent to the GPU (TASK-0169B P2, RFC-0101 §8c): a free slot acquired
//! like a present's, its bus address and bytes handed out for the GPU's binding and the cache
//! maintenance around the job, and the way to the glass — the splash's reveal while the splash
//! still holds (the GPU's first picture is what replaces the boot splash), a flip at the next
//! blank otherwise, the ring's rules unchanged.

use nexus_display_proto::layout;
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::types::Rect;

use super::{ring, DcDisplay};
use crate::backend::display::GpuCanvas;

impl DcDisplay {
    pub(super) fn canvas_begin(&mut self) -> Option<GpuCanvas> {
        let slot = self.ring.begin().ok()?;
        let row = ring::ROWS[slot];
        let Some(fb) = self.live.as_ref().and_then(|l| l.framebuffer.as_ref()) else {
            self.ring.discard();
            return None;
        };
        // A rung's job and its CPU checks both touch the slot: a readback and windowd's first
        // GPU frame in it maintain the caches first (5c-3 P4b).
        self.slot_gpu[slot] = true;
        self.slot_cpu[slot] = true;
        Some(GpuCanvas {
            row,
            bus: fb.plane_bus(row),
            len: layout::PLANE_BYTES,
            block: fb.cache_block(),
        })
    }

    pub(super) fn canvas_bytes(&mut self, row: u32) -> Option<&mut [u8]> {
        self.live.as_mut()?.framebuffer.as_mut()?.plane_bytes_mut(row)
    }

    pub(super) fn canvas_show(&mut self, row: u32) -> Result<(), GfxError> {
        let slot = ring::ROWS.iter().position(|&r| r == row).ok_or(GfxError::InvalidArgument)?;
        if self.ring.drawing() != Some(slot) {
            return Err(GfxError::CommandRejected);
        }
        let (w, h) = self.mode;
        self.ring.damage(Rect { x: 0, y: 0, width: w.into(), height: h.into() });
        if self.hold.holding() {
            // The GPU's picture is the switch from the splash.
            let took = self.reveal(row);
            self.hold.switched(took);
            if took {
                return self.ring.shown_now().map_err(|_| GfxError::ResourceExhausted);
            }
            self.ring.discard();
            return Err(GfxError::DeviceNotFound);
        }
        let Some(live) = self.live.as_ref() else {
            self.ring.discard();
            return Err(GfxError::DeviceNotFound);
        };
        if self.vblank.is_none() {
            if let Some(fb) = live.framebuffer.as_ref() {
                let _ = live.controller.flip_in_blank(&fb.plane_at(row, w, h));
            }
            return self.ring.shown_now().map_err(|_| GfxError::ResourceExhausted);
        }
        self.ring.drawn().map_err(|_| GfxError::ResourceExhausted)?;
        live.controller.vblank_enable();
        Ok(())
    }

    pub(super) fn canvas_discard(&mut self) {
        self.ring.discard();
    }
}
