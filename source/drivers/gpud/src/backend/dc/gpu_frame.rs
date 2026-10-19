// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A frame of windowd's drawn on the GPU into the ring's slot (TASK-0215 5c-3 P4b, RFC-0101 §8,
//! §8d). The GPU takes a present the frame plan covers (`RetainedLayers::covers`: the layer set,
//! the damage blits from the retained plane, the pointer — here the controller's layer); a
//! present with any other command is the CPU's, and so is a frame the GPU did not draw. The
//! device does not snoop the caches: before the GPU draws a slot the CPU wrote since, the slot's
//! lines are written back and dropped (a line written back after the GPU's pass would land over
//! its pixels); before the CPU reads a slot the GPU drew — the readback — its stale lines are
//! dropped. The scene starts from the wallpaper plane, or from the retained plane while a capture
//! freeze holds the frame (RFC-0095: the readback copied the frame there).

use nexus_display_proto::layout;
use nexus_gfx::command::buffer::Command;
use nexus_gfx::frame::RetainedLayers;

use super::{ring, DcDisplay};
use crate::gpu_context::{FrameGpu, FrameTarget};

impl DcDisplay {
    /// The running present drawn on the GPU into `slot`: whether it was.
    pub(super) fn draw_on_gpu(
        &mut self,
        frames: &mut dyn FrameGpu,
        commands: &[Command],
        slot: usize,
    ) -> bool {
        if !RetainedLayers::covers(commands) {
            return false;
        }
        let display = (u32::from(self.mode.0), u32::from(self.mode.1));
        let base_row = if self.frozen { layout::RETAINED_ROW } else { layout::WALLPAPER_ROW };
        let row = ring::ROWS[slot];
        let Some(fb) = self.live.as_ref().and_then(|l| l.framebuffer.as_ref()) else {
            return false;
        };
        if self.slot_cpu[slot] {
            fb.flush_plane(row);
        }
        let target = FrameTarget {
            layout_bus: fb.bus(),
            layout: fb.bytes(),
            block: fb.cache_block(),
            slot_row: row,
            display,
            base_row,
            base_dirty: self.base_dirty,
        };
        let drawn = frames.draw_frame(commands, &target);
        if drawn {
            self.slot_cpu[slot] = false;
            self.slot_gpu[slot] = true;
            self.base_dirty = false;
        }
        drawn
    }

    /// The plane on screen as the CPU is about to read it: its stale lines dropped when the GPU
    /// drew it.
    pub(super) fn front_for_cpu(&mut self) {
        let Some(slot) = self.ring.scanned() else { return };
        if !self.slot_gpu[slot] {
            return;
        }
        if let Some(fb) = self.live.as_ref().and_then(|l| l.framebuffer.as_ref()) {
            fb.flush_plane(ring::ROWS[slot]);
        }
    }
}
