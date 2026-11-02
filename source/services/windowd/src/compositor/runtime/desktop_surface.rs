// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! BOUNDARY: this file is the DESKTOP surface slice of the app-surface service: register the
//! declared `level: desktop` surface (the shell / greeter app-host) — its buffer is gpud's like
//! every app's (RFC-0114), composed chromeless as the base layer below all floating windows —
//! and route taps / wheel / acks on its dedicated channel. It shares the create/present/destroy
//! protocol contract of `app_window.rs`. MUST NOT grow chrome/layout — the shell owns every
//! pixel of the desktop surface itself.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use super::*;
use crate::client_surface::Delivery;
use nexus_display_proto::client_surface as wire;

impl DisplayServerRuntime {
    /// Registers the DESKTOP surface (declared `level: desktop` — the shell or greeter
    /// app-host): its own id and event channel, shown in the Desktop z-band (the base layer,
    /// below every floating window). gpud makes its buffer; the answer follows gpud's
    /// (RFC-0114 §3). `Some`: refused now — the answer to send at once.
    pub(super) fn create_desktop_surface(
        &mut self,
        c: &wire::SurfaceCreate,
        sender_sid: u64,
    ) -> Option<[u8; wire::SURFACE_BUFFERS_FRAME_LEN]> {
        let (width, height, nonce) = (c.width, c.height, c.nonce);
        let id = match self.client_surfaces.create(width, height, (c.rows, c.store_w), c.format) {
            Ok(id) => id,
            Err(status) => {
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: desktop surface create FAIL (status={status})"
                ));
                return Some(wire::encode_surface_buffers(&wire::Buffers::refused(status)));
            }
        };
        // RFC-0086: the desktop-surface owner is the ONLY sender the taskbar verbs
        // (OP_SURFACE_TASKBAR) accept — captured from the kernel recv metadata here, never from
        // payload bytes, once its surface exists. sid 0 (an unnamed spawn) stays 0 and everything
        // gated on it refuses.
        let previous_owner = core::mem::replace(&mut self.desktop_owner_sid, sender_sid);
        // A re-create without a destroy: the shell's surface retires like a destroyed one and
        // shows until this one presents.
        if let Some(old) = self.desktop_surface_id.replace(id) {
            self.retire_surface(super::surface_buffers::Owner::Desktop, old);
            // The replaced surface stays its owner's (the greeter a shell replaces).
            self.buffers.desktop_retiring_owner = previous_owner;
        }
        // The shell launch surfaced (greeter → shell swap): stop the wait ring.
        self.end_cursor_wait();
        if let Some(ch) = self.event_channel_for(nonce) {
            self.take_desktop_channel(ch);
            self.presentation.note_bound(crate::presentation_state::DESKTOP);
            self.desktop_pending_nonce = None;
            // RFC-0086: the feed is RETAINED state, not an event stream — a fresh (or
            // re-created) shell gets the whole window set now.
            self.windows_feed.owed = true;
        } else {
            // The desktop's OP_SURFACE_CREATE raced ahead of its event-channel attach: the bind
            // completes in that handler (and the buffer's answer with it), never dropped.
            self.desktop_pending_nonce = Some(nonce);
            let _ = debug_println(&alloc::format!(
                "WINDOWD: desktop bind deferred (channel not yet attached) nonce={nonce:#x}"
            ));
        }
        // A new surface declares its layers anew (RFC-0109).
        self.desktop_layers.clear();
        self.show_window(crate::window_scene::WindowId::Desktop);
        self.queue_full_frame_damage();
        let _ = debug_println(&alloc::format!(
            "WINDOWD: desktop surface created id={id} {width}x{height}"
        ));
        // A desktop surface smaller than the display (its pre-create rect ask raced): push the
        // full content rect so the app re-creates at display size — the base layer must cover
        // the screen.
        if u32::from(width) != self.mode.width || u32::from(height) != self.mode.height {
            let rect = wire::encode_surface_rect(
                0,
                0,
                self.mode.width.min(u32::from(u16::MAX)) as u16,
                self.mode.height.min(u32::from(u16::MAX)) as u16,
            );
            if let Some(slot) = self.desktop_channel {
                // The content rect is state: the newest wins while the client is behind.
                let _ = self.client_send(slot, &rect, Delivery::Latest);
            }
        }
        // The DSL shell is on — as an app-host-owned desktop surface (the in-process mount that
        // used to emit this is deleted, Umbau #17 2d).
        let _ = debug_println("systemui: dsl shell on");
        // gpud makes the buffer; its answer completes the create.
        self.request_surface_buffer(id, sender_sid, (width, c.rows));
        None
    }

    /// `channel` becomes the desktop's. Another client's desktop it replaces — the greeter's,
    /// after login — is let go on purpose (RFC-0112): its app is told and ends, instead of
    /// lingering for the session and taking the desktop back after a compositor restart.
    fn take_desktop_channel(&mut self, channel: u32) {
        let replaced = self.desktop_channel.replace(channel).filter(|old| *old != channel);
        #[cfg(nexus_env = "os")]
        if let Some(old) = replaced {
            if let Some(pos) = self.event_channels.iter().position(|e| e.1 == old) {
                self.event_channels.remove(pos);
                self.presentation.forget(pos);
            }
            self.close_client_channel(old);
        }
        #[cfg(not(nexus_env = "os"))]
        let _ = replaced;
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
        self.take_desktop_channel(slot);
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
            // A tap carries the press's facts, a MOVE or DRAG its samples (`motion.rs`).
            let (frame, n) = self.pointer_frame(id, kind, local_x, local_y);
            let Some(slot) = self.desktop_channel else {
                if kind == wire::INPUT_KIND_TAP {
                    let _ = debug_println("WINDOWD: FAIL desktop input (no event channel)");
                }
                return false;
            };
            let sent = self.client_send(slot, &frame[..n], Delivery::for_input(kind)).accepted();
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
