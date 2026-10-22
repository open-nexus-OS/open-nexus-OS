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
use crate::client_surface::Delivery;
use nexus_display_proto::client_surface as wire;

impl DisplayServerRuntime {
    /// Registers the DESKTOP surface (declared `level: desktop` — the shell or
    /// greeter app-host): own id + event channel + full-screen atlas band,
    /// shown in the Desktop z-band (composited as the base layer, below all
    /// floating windows). Fail-closed: no band → QUOTA, registration rolled back.
    pub(super) fn create_desktop_surface(
        &mut self,
        c: &wire::SurfaceCreate,
        vmo_slot: u32,
        sender_sid: u64,
    ) -> [u8; wire::SURFACE_ACK_FRAME_LEN] {
        let (width, height, nonce) = (c.width, c.height, c.nonce);
        // RFC-0086: the desktop-surface owner is the ONLY sender the taskbar
        // verbs (OP_SURFACE_TASKBAR) accept — captured from the kernel recv
        // metadata here, never from payload bytes. sid 0 (an unnamed spawn)
        // stays 0 and everything gated on it refuses.
        self.desktop_owner_sid = sender_sid;
        let id = match self.client_surfaces.create(
            width,
            height,
            (c.rows, c.store_w),
            c.format,
            vmo_slot,
        ) {
            Ok(id) => id,
            Err(status) => {
                let _ = super::app_window::nexus_abi_cap_close(vmo_slot);
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: desktop surface create FAIL (status={status})"
                ));
                return wire::encode_surface_ack(wire::OP_SURFACE_CREATE, status, 0);
            }
        };
        // The desktop's base: the display's width and height — kept across re-creates while it
        // fits. Its store (RFC-0109) is a region of its own below.
        let need = u32::from(height).max(self.mode.height);
        if let Some(band) = self.desktop_band.filter(|b| b.height < need) {
            self.atlas_alloc.free(band);
            self.desktop_band = None;
        }
        if self.desktop_band.is_none() {
            let Some(band) = self.atlas_alloc.alloc(self.mode.width, need) else {
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: desktop surface FAIL atlas (need={}x{} rows_remaining={})",
                    self.mode.width,
                    need,
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
        let status = self.place_desktop_store(id, c);
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
            self.presentation.note_bound(crate::presentation_state::DESKTOP);
            self.desktop_pending_nonce = None;
            // RFC-0086: the feed is RETAINED state, not an event stream — a
            // fresh (or re-created) shell gets the whole window set now.
            self.windows_feed.owed = true;
        } else {
            // The event-channel attach for this nonce has not arrived yet — the
            // desktop's OP_SURFACE_CREATE raced ahead of attach_app_event_channel.
            // Defer the bind (complete it in that handler) instead of dropping the
            // channel, which used to leave the desktop stuck at its fallback size.
            self.desktop_pending_nonce = Some(nonce);
            let _ = debug_println(&alloc::format!(
                "WINDOWD: desktop bind deferred (channel not yet attached) nonce={nonce:#x}"
            ));
        }
        self.desktop_dirty = true;
        self.desktop_damage = super::surface_layers::DamageRows::FULL;
        // A new surface declares its layers anew (RFC-0109).
        self.desktop_layers.clear();
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
        wire::encode_surface_ack(wire::OP_SURFACE_CREATE, status, id)
    }

    /// The desktop's store (RFC-0109): its used width × its rows, wherever the atlas has room — so
    /// a store that grows (the launcher opening) never needs the rows next to the desktop's base.
    /// Without room the surface is its base alone and the client is told (`NO_STORE`): it paints
    /// its scroll layers flat.
    fn place_desktop_store(&mut self, id: u32, c: &wire::SurfaceCreate) -> u8 {
        if let Some(store) = self.desktop_store.take() {
            self.atlas_alloc.free(store);
        }
        if c.rows <= c.height {
            return wire::SURFACE_STATUS_OK;
        }
        let (w, h) = (u32::from(c.store_w), u32::from(c.rows - c.height));
        self.desktop_store = self.atlas_alloc.alloc(w, h);
        if self.desktop_store.is_some() {
            return wire::SURFACE_STATUS_OK;
        }
        self.client_surfaces.drop_store(id);
        let _ = debug_println(&alloc::format!(
            "WINDOWD: desktop store FAIL atlas (need={w}x{h}): flat scroll layers"
        ));
        wire::SURFACE_STATUS_NO_STORE
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
        self.presentation.note_bound(crate::presentation_state::DESKTOP);
        self.desktop_pending_nonce = None;
        let rect = wire::encode_surface_rect(
            0,
            0,
            self.mode.width.min(u32::from(u16::MAX)) as u16,
            self.mode.height.min(u32::from(u16::MAX)) as u16,
        );
        // The content rect is state: the newest wins while the client is behind (ADR-0075).
        let _ = self.client_send(slot, &rect, Delivery::Latest);
        // RFC-0086: same retained re-push as the non-deferred bind path.
        self.windows_feed.owed = true;
        let _ = debug_println("WINDOWD: desktop bind completed (late channel attach)");
    }

    /// Sends a frame on the DESKTOP channel (survives a desktop destroy — the
    /// channel stays bound for the re-create, so the destroy-ack still lands).
    pub(crate) fn send_desktop_ack(&mut self, frame: &[u8]) -> bool {
        #[cfg(nexus_env = "os")]
        {
            let Some(slot) = self.desktop_channel else { return false };
            // The client waits on this ack (TASK-0306): ordered and kept, never a park.
            if self.client_send(slot, frame, Delivery::Ack).accepted() {
                return true;
            }
            let _ = debug_println("WINDOWD: FAIL desktop event send");
            false
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = frame;
            false
        }
    }

    /// Copies the rows the desktop's presents named out of its VMO into its region — chromeless
    /// (the shell owns every pixel), its base's rows and its store's (RFC-0109). The same
    /// damage-bounded copy as a window's (ADR-0042).
    pub(super) fn render_desktop_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else { return Ok(()) };
        let Some(band) = self.desktop_band else { return Ok(()) };
        let Some(id) = self.desktop_surface_id else { return Ok(()) };
        let Some(client) = self.client_surfaces.get_by_id(id).copied() else { return Ok(()) };
        let damage =
            core::mem::replace(&mut self.desktop_damage, super::surface_layers::DamageRows::NONE);
        let regions = crate::surface_regions::SurfaceRegions::of(&client, band, self.desktop_store);
        self.copy_surface_rows(handle, &client, &regions, &damage)
    }

    /// Routes a tap that fell through to the DESKTOP surface to its owning
    /// app-host (the shell) — same OP_SURFACE_INPUT contract as window bodies,
    /// surface-local (the desktop is full-screen at the origin).
    pub(crate) fn send_desktop_input(&mut self, local_x: i32, local_y: i32) {
        self.send_desktop_input_kind(wire::INPUT_KIND_TAP, local_x, local_y);
    }

    /// `send_desktop_input` for any input kind; `true` when the frame went
    /// out. Taps keep their honest routed/FAIL markers; MOVE/LEAVE/DRAG are
    /// frame-rate traffic and stay silent (a marker per move would flood the UART).
    pub(crate) fn send_desktop_input_kind(&mut self, kind: u8, local_x: i32, local_y: i32) -> bool {
        #[cfg(nexus_env = "os")]
        {
            let Some(id) = self.desktop_surface_id else { return false };
            let (x, y) = (local_x.max(0) as u16, local_y.max(0) as u16);
            let frame = wire::encode_surface_input(id, kind, x, y);
            let Some(slot) = self.desktop_channel else {
                if kind == wire::INPUT_KIND_TAP {
                    let _ = debug_println("WINDOWD: FAIL desktop input (no event channel)");
                }
                return false;
            };
            let sent = self.client_send(slot, &frame, Delivery::for_input(kind)).accepted();
            if sent && kind == wire::INPUT_KIND_TAP && super::input_hover::first_tap_routed(true) {
                let _ = debug_println("WINDOWD: desktop input routed");
            } else if !sent && kind == wire::INPUT_KIND_TAP {
                let _ = debug_println("WINDOWD: FAIL desktop input send");
            }
            sent
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (kind, local_x, local_y);
            false
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
                let kind = nexus_display_proto::client_surface::INPUT_KIND_WHEEL;
                let _ = self.client_send(slot, &frame, Delivery::for_input(kind));
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (local_x, wire_delta);
        }
    }
}
