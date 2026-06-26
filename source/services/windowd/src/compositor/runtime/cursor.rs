// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — cursor upload + HW-overlay move + SW damage.
//! OWNERS: @ui
//! STATUS: Experimental
//!
//! Split out of `runtime/mod.rs` (TASK-0063 modularization): the
//! `DisplayServerRuntime` methods for the pointer. `upload_cursor_bitmap_to_gpud`
//! arms the cursor (HW overlay on the cursor virtqueue, else software BlendCursor);
//! `send_cursor_move_to_gpud` is the reactive HW hot path (fire-and-forget, no
//! present). `merged_cursor_damage_rect`/`queue_cursor_damage` are the software
//! fallback's old∪new cursor-rect damage. Child module of `runtime`, so it reads
//! the runtime's private state; `pub(super)` so the parent + siblings call it.

use super::*;

impl DisplayServerRuntime {
    pub(super) fn upload_cursor_bitmap_to_gpud(&mut self) {
        if !self.ensure_gpud_client() {
            return;
        }
        let bitmap = crate::assets::CURSOR_LEFT_PTR_BGRA;
        let w = crate::assets::CURSOR_LEFT_PTR_WIDTH;
        let h = crate::assets::CURSOR_LEFT_PTR_HEIGHT;
        let bgra_len = (w as usize).saturating_mul(h as usize).saturating_mul(4);
        if bgra_len == 0 || bgra_len > bitmap.len() {
            return;
        }
        // Frame: [opcode(1)] + [w(4)] + [h(4)] + [hot_x(4)] + [hot_y(4)] + [bgra]
        let total = 17usize.saturating_add(bgra_len);
        let mut frame: alloc::vec::Vec<u8> = alloc::vec![0u8; total];
        frame[0] = GPU_UPLOAD_CURSOR_OP;
        frame[1..5].copy_from_slice(&w.to_le_bytes());
        frame[5..9].copy_from_slice(&h.to_le_bytes());
        frame[9..13]
            .copy_from_slice(&(crate::assets::CURSOR_HOTSPOT_X.max(0) as u32).to_le_bytes());
        frame[13..17]
            .copy_from_slice(&(crate::assets::CURSOR_HOTSPOT_Y.max(0) as u32).to_le_bytes());
        frame[17..total].copy_from_slice(&bitmap[..bgra_len]);
        // Blocking round-trip. The upload reply is magic-tagged (0xC0DE_000x);
        // any other reply seen while waiting is an in-flight present ack (e.g.
        // the handoff present) and is accounted as such instead of being
        // mistaken for the cursor reply.
        self.drain_gpud_replies();
        let Some(client) = self.gpud_client.as_ref() else {
            return;
        };
        if client.send(&frame, Wait::Blocking).is_err() {
            return;
        }
        // Gate 2: cursor reply magics from the shared wire SSOT.
        const CURSOR_REPLY_HW: u32 = nexus_display_proto::CURSOR_REPLY_HW;
        const CURSOR_REPLY_SW: u32 = nexus_display_proto::CURSOR_REPLY_SW;
        // virgl GL scanout: gpud draws a procedural cursor at its pointer pos —
        // we must ship moves + a present, not a software BlendCursor.
        const CURSOR_REPLY_GL: u32 = nexus_display_proto::CURSOR_REPLY_GL;
        let mut cursor_flags: Option<u32> = None;
        let mut present_acks_seen = 0u32;
        let mut reply = [0u8; 8];
        for _ in 0..4 {
            match client.recv_into(Wait::Blocking, &mut reply) {
                Ok(n) if n >= 1 && reply[0] == GPUD_STATUS_OK => {
                    let payload = if n >= 5 {
                        u32::from_le_bytes([reply[1], reply[2], reply[3], reply[4]])
                    } else {
                        0
                    };
                    if payload == CURSOR_REPLY_HW
                        || payload == CURSOR_REPLY_SW
                        || payload == CURSOR_REPLY_GL
                    {
                        cursor_flags = Some(payload);
                        break;
                    }
                    if n >= 5 {
                        present_acks_seen += 1;
                    }
                }
                _ => break,
            }
        }
        for _ in 0..present_acks_seen {
            self.note_present_completed();
        }
        match cursor_flags {
            Some(flags) => {
                let hw = flags == CURSOR_REPLY_HW;
                let gl = flags == CURSOR_REPLY_GL;
                let changed = hw != self.hw_cursor_active || gl != self.gl_cursor_active;
                self.hw_cursor_active = hw;
                self.gl_cursor_active = gl;
                // Only log on a state TRANSITION — cursor replies can arrive per
                // move, and an unconditional print here would flood the UART log.
                if changed {
                    let _ = debug_println(if hw {
                        "windowd: hw cursor on"
                    } else if gl {
                        "windowd: gl procedural cursor on"
                    } else {
                        "windowd: cursor bitmap uploaded (sw)"
                    });
                }
                if gl {
                    // Procedural GL cursor: place it once at the current pointer.
                    // Moves then ship OP_MOVE_CURSOR + a present-damage (handled
                    // in `apply_input_state`) so the build-up present re-renders it.
                    self.send_cursor_move_to_gpud();
                }
                if hw {
                    // Place the overlay at the current pointer position.
                    self.send_cursor_move_to_gpud();
                    // The first present blended the software cursor into the
                    // display plane before the overlay was armed — restore that
                    // region from Plane 1 once so the baked sprite disappears.
                    let cx = (self.state.cursor_x - crate::assets::CURSOR_HOTSPOT_X).max(0) as u32;
                    let cy = (self.state.cursor_y - crate::assets::CURSOR_HOTSPOT_Y).max(0) as u32;
                    self.queue_gpu_blit_rect(DamageRect {
                        x: cx,
                        y: cy,
                        width: self.cursor_width.max(1),
                        height: self.cursor_height.max(1),
                    });
                }
            }
            _ => {
                let _ = debug_println("windowd: cursor upload failed");
            }
        }
    }

    /// Upload the real Lucide icon sprite to gpud once (TASK #61 "real icon
    /// layer"). gpud composites it as a GPU sprite layer on the virgl scanout,
    /// reusing the cursor's texture/sprite-layer plumbing. Blocking send (a
    /// one-shot at startup) + a brief reply drain so the 1-byte ack doesn't leak
    /// into the present pipeline; positioned near the top-left of the desktop.
    #[allow(dead_code)] // retained for the P3 topbar app icon; test sprite retired
    pub(super) fn upload_icon_to_gpud(&mut self) {
        if !self.ensure_gpud_client() {
            return;
        }
        let bitmap = crate::assets::SHELL_ICON_BGRA;
        let w = crate::assets::SHELL_ICON_WIDTH;
        let h = crate::assets::SHELL_ICON_HEIGHT;
        let bgra_len = (w as usize).saturating_mul(h as usize).saturating_mul(4);
        if bgra_len == 0 || bgra_len > bitmap.len() {
            return;
        }
        let dst_x: u32 = 48;
        let dst_y: u32 = 48;
        // On-screen size = the logical icon size; the texture is rendered at 2×
        // (supersampled) and GPU-downscaled to this, so it's crisp.
        let dst_w: u32 = crate::assets::SHELL_ICON_LOGICAL;
        let dst_h: u32 = crate::assets::SHELL_ICON_LOGICAL;
        // Frame: [op] + [tex_w] + [tex_h] + [dst_x] + [dst_y] + [dst_w] + [dst_h] + [bgra]
        let total = 25usize.saturating_add(bgra_len);
        let mut frame: alloc::vec::Vec<u8> = alloc::vec![0u8; total];
        frame[0] = GPU_UPLOAD_ICON_OP;
        frame[1..5].copy_from_slice(&w.to_le_bytes());
        frame[5..9].copy_from_slice(&h.to_le_bytes());
        frame[9..13].copy_from_slice(&dst_x.to_le_bytes());
        frame[13..17].copy_from_slice(&dst_y.to_le_bytes());
        frame[17..21].copy_from_slice(&dst_w.to_le_bytes());
        frame[21..25].copy_from_slice(&dst_h.to_le_bytes());
        frame[25..total].copy_from_slice(&bitmap[..bgra_len]);
        self.drain_gpud_replies();
        let Some(client) = self.gpud_client.as_ref() else {
            return;
        };
        if client.send(&frame, Wait::Blocking).is_err() {
            let _ = debug_println("windowd: shell icon upload failed");
            return;
        }
        // Drain the 1-byte status ack (present acks seen meanwhile are accounted).
        let mut reply = [0u8; 8];
        for _ in 0..4 {
            match client.recv_into(Wait::Blocking, &mut reply) {
                Ok(n) if n >= 1 && reply[0] == GPUD_STATUS_OK => break,
                _ => break,
            }
        }
        let _ = debug_println("windowd: shell icon uploaded");
    }

    /// Hardware-cursor hot path: ship the pointer position to gpud's cursor
    /// queue. Fire-and-forget — the 1-byte ack is drained asynchronously and
    /// never touches the present pipeline. No composite, no blit, no present.
    pub(super) fn send_cursor_move_to_gpud(&mut self) {
        let x = self.state.cursor_x.clamp(0, self.mode.width.saturating_sub(1) as i32);
        let y = self.state.cursor_y.clamp(0, self.mode.height.saturating_sub(1) as i32);
        let mut frame = [0u8; 9];
        frame[0] = GPU_MOVE_CURSOR_OP;
        frame[1..5].copy_from_slice(&x.to_le_bytes());
        frame[5..9].copy_from_slice(&y.to_le_bytes());
        let _ = self.send_gpud_fire_forget(&frame);
    }

    pub(super) fn merged_cursor_damage_rect(
        &self,
        old_cursor_x: i32,
        old_cursor_y: i32,
        new_cursor_x: i32,
        new_cursor_y: i32,
    ) -> Option<DamageRect> {
        let old_rect = cursor_damage_rect(
            old_cursor_x,
            old_cursor_y,
            self.cursor_width,
            self.cursor_height,
            self.mode.width,
            self.mode.height,
        );
        let new_rect = cursor_damage_rect(
            new_cursor_x,
            new_cursor_y,
            self.cursor_width,
            self.cursor_height,
            self.mode.width,
            self.mode.height,
        );
        match (old_rect, new_rect) {
            (Some(old_rect), Some(new_rect)) => Some(old_rect.merge(new_rect)),
            (Some(rect), None) | (None, Some(rect)) => Some(rect),
            (None, None) => None,
        }
    }

    pub(super) fn queue_cursor_damage(
        &mut self,
        old_cursor_x: i32,
        old_cursor_y: i32,
        new_cursor_x: i32,
        new_cursor_y: i32,
    ) {
        if let Some(rect) =
            self.merged_cursor_damage_rect(old_cursor_x, old_cursor_y, new_cursor_x, new_cursor_y)
        {
            // Cursor-only damage: no CPU recomposite. Merge into the dedicated
            // cursor track so flush blits it from the (cursor-free) retained plane
            // and overlays the pointer — pure GPU, no panel/text re-render.
            self.tile_map.mark_rect(rect);
            self.pending_cursor_rect = Some(match self.pending_cursor_rect {
                Some(existing) => existing.merge(rect),
                None => rect,
            });
        }
    }
}
