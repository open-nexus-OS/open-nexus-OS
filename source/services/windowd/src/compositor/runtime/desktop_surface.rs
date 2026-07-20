// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! BOUNDARY: this file is the DESKTOP surface slice of the app-surface service
//! (a pure move out of `app_window.rs`, no behavior change): register the
//! declared `level: desktop` surface (the shell / greeter app-host) into its
//! own full-screen atlas band, blit it chromeless row-for-row, route taps /
//! wheel / acks on its dedicated channel. It shares the create/present/destroy
//! protocol contract of `app_window.rs`; the desktop is the base layer (below
//! all floating windows). MUST NOT grow chrome/layout — the shell owns every
//! pixel of the desktop surface itself.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! ADR: docs/adr/0042-cross-process-surface-transport.md

use super::*;
use nexus_display_proto::client_surface as wire;

impl DisplayServerRuntime {
    /// Registers the DESKTOP surface (declared `level: desktop` — the shell or
    /// greeter app-host): own id + event channel + full-screen atlas band,
    /// shown in the Desktop z-band (composited as the base layer, below all
    /// floating windows). Fail-closed: no band → QUOTA, registration rolled back.
    pub(super) fn create_desktop_surface(
        &mut self,
        width: u16,
        height: u16,
        format: u8,
        vmo_slot: u32,
        nonce: u64,
    ) -> [u8; wire::SURFACE_ACK_FRAME_LEN] {
        let id = match self.client_surfaces.create(width, height, format, vmo_slot) {
            Ok(id) => id,
            Err(status) => {
                let _ = super::app_window::nexus_abi_cap_close(vmo_slot);
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: desktop surface create FAIL (status={status})"
                ));
                return wire::encode_surface_ack(wire::OP_SURFACE_CREATE, status, 0);
            }
        };
        if self.desktop_band.is_none() {
            let Some(band) = self.atlas_alloc.alloc(self.mode.width, self.mode.height) else {
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: desktop surface FAIL atlas (need={}x{} rows_remaining={})",
                    self.mode.width,
                    self.mode.height,
                    self.atlas_alloc.rows_remaining()
                ));
                let _ = self.client_surfaces.destroy(id);
                let _ = super::app_window::nexus_abi_cap_close(vmo_slot);
                return wire::encode_surface_ack(
                    wire::OP_SURFACE_CREATE,
                    wire::SURFACE_STATUS_QUOTA,
                    0,
                );
            };
            self.desktop_band = Some(band);
        }
        // A relaunched shell replaces the previous desktop surface (its VMO cap
        // was already released via destroy; ids never alias — monotonic).
        let fresh = self.desktop_surface_id != Some(id);
        self.desktop_surface_id = Some(id);
        // The shell launch surfaced (greeter → shell swap): stop the wait ring.
        if fresh {
            self.end_cursor_wait();
        }
        #[cfg(nexus_env = "os")]
        if let Some(ch) = self.event_channel_for(nonce) {
            self.desktop_channel = Some(ch);
            self.desktop_pending_nonce = None;
        } else {
            // The event-channel attach for this nonce has not arrived yet — the
            // desktop's OP_SURFACE_CREATE raced ahead of attach_app_event_channel.
            // Defer the bind (complete it in that handler) instead of dropping the
            // channel, which used to leave the desktop stuck at its fallback size.
            self.desktop_pending_nonce = Some(nonce);
            let _ = debug_println("WINDOWD: desktop bind deferred (channel not yet attached)");
        }
        self.desktop_dirty = true;
        self.desktop_dirty_rows = (0, u32::MAX);
        self.show_window(crate::window_scene::WindowId::Desktop);
        self.queue_full_frame_damage();
        let _ = debug_println(&alloc::format!(
            "WINDOWD: desktop surface created id={id} {width}x{height}"
        ));
        // A desktop surface smaller than the display (its pre-create rect ask
        // raced): push the full content rect so the app re-creates at display
        // size — the base layer must cover the screen.
        if u32::from(width) != self.mode.width || u32::from(height) != self.mode.height {
            let rect = wire::encode_surface_rect(
                0,
                0,
                self.mode.width.min(u32::from(u16::MAX)) as u16,
                self.mode.height.min(u32::from(u16::MAX)) as u16,
            );
            #[cfg(nexus_env = "os")]
            if let Some(slot) = self.desktop_channel {
                let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, rect.len() as u32);
                let _ = nexus_abi::ipc_send_v1(slot, &hdr, &rect, nexus_abi::IPC_SYS_NONBLOCK, 0);
            }
            #[cfg(not(nexus_env = "os"))]
            let _ = rect;
        }
        // The DSL shell is on — as an app-host-owned desktop surface (the
        // in-process mount that used to emit this is deleted, Umbau #17 2d).
        let _ = debug_println("systemui: dsl shell on");
        wire::encode_surface_ack(wire::OP_SURFACE_CREATE, wire::SURFACE_STATUS_OK, id)
    }

    /// The current desktop surface id, for ack-ownership decisions taken
    /// BEFORE a handler mutates the bookkeeping (destroy clears the id).
    pub(crate) fn desktop_surface_id_for_ack(&self) -> Option<u32> {
        self.desktop_surface_id
    }

    /// Complete a desktop bind that was deferred because the surface-create raced
    /// ahead of its event-channel attach (see `create_desktop_surface`). Called
    /// from `attach_app_event_channel` when the matching nonce lands: binds the
    /// channel and pushes the full-screen content rect that the create path had
    /// to skip, so the app re-creates the desktop at display size instead of
    /// staying stuck at its pre-attach fallback surface.
    #[cfg(nexus_env = "os")]
    pub(super) fn complete_deferred_desktop_bind(&mut self, nonce: u64, slot: u32) {
        if self.desktop_pending_nonce != Some(nonce) {
            return;
        }
        self.desktop_channel = Some(slot);
        self.desktop_pending_nonce = None;
        let rect = wire::encode_surface_rect(
            0,
            0,
            self.mode.width.min(u32::from(u16::MAX)) as u16,
            self.mode.height.min(u32::from(u16::MAX)) as u16,
        );
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, rect.len() as u32);
        let _ = nexus_abi::ipc_send_v1(slot, &hdr, &rect, nexus_abi::IPC_SYS_NONBLOCK, 0);
        let _ = debug_println("WINDOWD: desktop bind completed (late channel attach)");
    }

    /// Sends a frame on the DESKTOP channel (survives a desktop destroy — the
    /// channel stays bound for the re-create, so the destroy-ack still lands).
    pub(crate) fn send_desktop_ack(&mut self, frame: &[u8]) -> bool {
        #[cfg(nexus_env = "os")]
        {
            let Some(slot) = self.desktop_channel else { return false };
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
            match nexus_abi::ipc_send_v1(slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
                Ok(_) => true,
                Err(_) => {
                    let _ = debug_println("WINDOWD: FAIL desktop event send");
                    true // channel exists — no shared-endpoint fallback
                }
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = frame;
            false
        }
    }

    /// Blits the DESKTOP surface out of its VMO into the full-screen desktop
    /// band — chromeless, row-for-row (the shell owns every pixel). Same
    /// bounded damage-blit as the floating window body (ADR-0042).
    pub(super) fn render_desktop_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else { return Ok(()) };
        let Some(band) = self.desktop_band else { return Ok(()) };
        let Some(id) = self.desktop_surface_id else { return Ok(()) };
        let Some(client) = self.client_surfaces.get_by_id(id).copied() else { return Ok(()) };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let w = (client.width as u32).min(band.width).min(self.mode.width);
        let h = (client.height as u32).min(band.height).min(self.mode.height);
        let row_bytes = w as usize * 4;
        let src_stride = client.width as usize * 4;
        // Damage-limited: only the rows the client actually presented since
        // the last blit (union span). Empty span = nothing to copy.
        let (r0, r1) = self.desktop_dirty_rows;
        self.desktop_dirty_rows = (u32::MAX, 0);
        if r0 >= r1 {
            return Ok(());
        }
        let y_end = h.min(r1);
        for y in r0.min(y_end)..y_end {
            let row = &mut self.band_scratch[0..stride];
            #[cfg(nexus_env = "os")]
            {
                let src_off = y as usize * src_stride;
                if nexus_abi::vmo_read(client.vmo_slot, src_off, &mut row[..row_bytes]).is_err() {
                    return Err(WindowdError::BufferLengthMismatch);
                }
                let dst = (band.abs_row + y) as usize * stride + band.x as usize * 4;
                nexus_abi::vmo_write(handle, dst, &row[..row_bytes])
                    .map_err(|_| WindowdError::BufferLengthMismatch)?;
            }
            #[cfg(not(nexus_env = "os"))]
            {
                let _ = (row, src_stride, handle, y);
            }
        }
        Ok(())
    }

    /// Routes a tap that fell through to the DESKTOP surface to its owning
    /// app-host (the shell) — same OP_SURFACE_INPUT contract as window bodies,
    /// surface-local (the desktop is full-screen at the origin).
    pub(crate) fn send_desktop_input(&mut self, local_x: i32, local_y: i32) {
        self.send_desktop_input_kind(wire::INPUT_KIND_TAP, local_x, local_y);
    }

    /// `send_desktop_input` for any input kind. Taps keep their honest
    /// routed/FAIL markers; MOVE/LEAVE are frame-rate hover traffic and stay
    /// silent (a marker per move would flood the UART).
    pub(crate) fn send_desktop_input_kind(&mut self, kind: u8, local_x: i32, local_y: i32) {
        #[cfg(nexus_env = "os")]
        {
            let Some(id) = self.desktop_surface_id else { return };
            let (x, y) = (local_x.max(0) as u16, local_y.max(0) as u16);
            let frame = wire::encode_surface_input(id, kind, x, y);
            let Some(slot) = self.desktop_channel else {
                if kind == wire::INPUT_KIND_TAP {
                    let _ = debug_println("WINDOWD: FAIL desktop input (no event channel)");
                }
                return;
            };
            if super::app_window::send_input_frame(slot, &frame, kind == wire::INPUT_KIND_TAP) {
                if kind == wire::INPUT_KIND_TAP {
                    let _ = debug_println("WINDOWD: desktop input routed");
                }
            } else if kind == wire::INPUT_KIND_TAP {
                let _ = debug_println("WINDOWD: FAIL desktop input send");
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (kind, local_x, local_y);
        }
    }

    /// [`Self::send_app_wheel`] for the DESKTOP surface (shell launcher grid).
    pub(crate) fn send_desktop_wheel(&mut self, local_x: u16, wire_delta: u16) {
        #[cfg(nexus_env = "os")]
        {
            let Some(id) = self.desktop_surface_id else { return };
            let frame = nexus_display_proto::client_surface::encode_surface_input(
                id,
                nexus_display_proto::client_surface::INPUT_KIND_WHEEL,
                local_x,
                wire_delta,
            );
            if let Some(slot) = self.desktop_channel {
                let _ = super::app_window::send_input_frame(slot, &frame, false);
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (local_x, wire_delta);
        }
    }
}
