// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! BOUNDARY: this file is the SERVICE side of the app surface — registration,
//! present/ack flow control and the event channel (theme/rect pushes). It MUST NOT grow window-chrome/sizing/resize/
//! decoration logic — that is the `ui/widgets/window` widget's job (the frame
//! it currently opens is the legacy `ShellWindow`, being retired, see
//! windows-as-widgets.md). Keep this to: move surface bytes, route input,
//! push geometry/theme. Chrome + layout live in the widget + scene graph.
//!
//! CONTEXT: windowd compositor runtime — the cross-process app window (ADR-0042; by reference
//! since RFC-0114): `SURFACE_CREATE` registers the surface, opens its window and has gpud make
//! its buffer (`surface_buffers.rs`); `SURFACE_PRESENT` makes the drawn slot what the frames
//! read and acks it once the other slot is free; `SURFACE_DESTROY` retires the surface until its
//! successor presents. windowd moves no pixel: the GPU reads the app's own memory. Apps get a
//! buffer and events — never scene-graph or atlas access.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: bookkeeping host-tested in `crate::client_surface`; the lanes' markers
//! (`WINDOWD: surface …`, `windowd: compose copies=0 (`).
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use super::*;
use crate::client_surface::Delivery;
use nexus_display_proto::client_surface as wire;

/// Window bounds: the pool reserve + `ShellWindow` frame are sized for the
/// LARGEST allowed surface; smaller surfaces render into the top-left of the
/// body. (`crate::client_surface` enforces the surface-size bounds.)
pub(crate) const APP_WIN_MAX_W: u32 = crate::client_surface::MAX_SURFACE_W as u32;
pub(crate) const APP_WIN_MAX_H: u32 = crate::client_surface::MAX_SURFACE_H as u32;
/// The frame of a window that declares no size (`mode: auto`): the WM's small utility window
/// (TASK-0336: the WM owns every window's geometry; app-host's probe size before).
pub(crate) const AUTO_WINDOW: (u32, u32) = (320, 270);

impl DisplayServerRuntime {
    /// `SURFACE_CREATE` (v2, RFC-0114 §3): validate and register the surface, show its window,
    /// and have gpud make its buffer — the answer (`OP_SURFACE_BUFFERS`, the buffer's capability
    /// moved along) follows gpud's (`on_surface_allocated`). `Some`: refused now — the answer to
    /// send at once.
    pub(crate) fn handle_surface_create(
        &mut self,
        frame: &[u8],
        moved_cap: Option<u32>,
        sender_sid: u64,
    ) -> Option<[u8; wire::SURFACE_BUFFERS_FRAME_LEN]> {
        let refused = |status| Some(wire::encode_surface_buffers(&wire::Buffers::refused(status)));
        // No capability rides with a create any more: one that does is closed, never kept.
        if let Some(cap) = moved_cap {
            let _ = nexus_abi_cap_close(cap);
            let _ = debug_println("WINDOWD: surface create FAIL (a capability rode along)");
            return refused(wire::SURFACE_STATUS_MALFORMED);
        }
        let Some(c) = wire::decode_surface_create(frame) else {
            return refused(wire::SURFACE_STATUS_MALFORMED);
        };
        // Declarative routing at CREATE: the connecting app's presentation (its carried intent
        // ⟂ policy) decides the role — resolved from THIS frame's values, never from stored
        // state. A DESKTOP-role surface (shell/greeter) has its own id, channel and layer, apart
        // from the floating windows.
        let (width, height, style, level, mode, resizable) =
            (c.width, c.height, c.style, c.level, c.mode, c.resizable);
        let nonce = c.nonce;
        let presentation = crate::surface_presentation::WindowPresentation::resolve(
            style,
            level,
            mode,
            resizable,
            self.windowing_policy,
        );
        if presentation.role == crate::window_scene::WindowRole::Desktop {
            return self.create_desktop_surface(&c, sender_sid);
        }
        // Multi-window (RFC-0065): the create binds to the slot ALREADY holding this client's
        // event channel (a re-create resumes in place), else to the owner's GHOST (a rejoin after
        // a compositor restart, RFC-0112), else to a free slot — never another app's window.
        let bound_idx = self
            .event_channel_for(nonce)
            .and_then(|ch| self.apps.iter().position(|a| a.event_channel == Some(ch)))
            .or_else(|| self.take_ghost_slot(sender_sid, (width, height)));
        // FRESH launch (vs a re-create resuming its slot): only a fresh window plays the open
        // transition. A slot reserved by the launch's starting surface is a fresh launch too.
        let fresh_launch = bound_idx.is_none_or(|i| self.apps[i].starting);
        let Some(idx) = bound_idx.or_else(|| self.take_app_slot()) else {
            let _ = debug_println("WINDOWD: surface create FAIL (no memory for a window slot)");
            return refused(wire::SURFACE_STATUS_QUOTA);
        };
        // A re-create without a destroy: the window's surface retires like a destroyed one.
        if let Some(old) = self.apps[idx].surface_id.take() {
            self.retire_surface(super::surface_buffers::Owner::App(idx), old);
        }
        // The starting surface stays until the app's first frame (`handle_surface_present`).
        let was_starting = self.apps[idx].starting;
        if fresh_launch {
            self.end_cursor_wait();
            self.apps[idx].reset_wm_state(); // a NEW window follows its declared intent
        }
        let wid = crate::window_scene::WindowId::App(idx as u16);
        // The declared intent rides ATOMICALLY on the create frame; it binds to THIS slot only.
        self.apps[idx].intent_style = style;
        self.apps[idx].intent_level = level;
        self.apps[idx].intent_mode = mode;
        self.apps[idx].intent_resizable = resizable;
        self.apps[idx].intent_min = (c.size.min_width, c.size.min_height);
        self.apps[idx].owner_sid = sender_sid;
        // A new surface declares its layers anew (RFC-0109): nothing of the last one's survives.
        self.reset_surface_state(idx);
        let id = match self.client_surfaces.create(width, height, (c.rows, c.store_w), c.format) {
            Ok(id) => id,
            Err(status) => {
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: surface create FAIL (status={status})"
                ));
                return refused(status);
            }
        };
        self.apps[idx].surface_id = Some(id);
        if let Some(ch) = self.event_channel_for(nonce) {
            self.apps[idx].event_channel = Some(ch);
        } else {
            let _ = debug_println("WINDOWD: FAIL surface bind (no channel for nonce)");
        }
        // The window FRAME follows the surface's size (windows-as-widgets P3.1): no full-screen
        // frame for a small window, no oversized shadow. Chrome per intent.
        if self.app_presentation(idx).docked_bottom {
            // OVERLAY dock (OSK): bottom edge, full width, no chrome.
            let h = u32::from(height);
            #[allow(clippy::cast_possible_wrap)]
            self.apps[idx].win.set_frame(
                0,
                self.mode.height.saturating_sub(h) as i32,
                self.mode.width,
                h,
            );
        } else if self.app_presentation(idx).full_screen || self.windows.is_fullscreen(wid) {
            let (y, h) = self.full_surface_frame(idx);
            self.apps[idx].win.set_frame(0, y, self.mode.width, h);
        } else {
            self.frame_floating(idx, (u32::from(width), u32::from(height)));
        }
        self.open_app_window(idx);
        // Track C3: a FRESH floating window fades+scales in; a starting surface ran the enter
        // spring already (TASK-0336) and the app's frame takes the window over as it is.
        if fresh_launch
            && !was_starting
            && !self.app_presentation(idx).full_screen
            && !self.windows.is_fullscreen(wid)
        {
            self.start_open_transition(idx);
        }
        // A RE-CREATE can shrink the frame: the whole scene again (no stale vacated area).
        if !fresh_launch {
            self.queue_full_frame_damage();
        }
        let _ = debug_println(&alloc::format!("WINDOWD: surface created id={id} {width}x{height}"));
        // gpud makes the buffer; its answer completes the create.
        self.request_surface_buffer(id, sender_sid, (width, c.rows));
        None
    }

    /// Sends a frame on the event channel bound to `nonce` — the CREATE-ack
    /// route: the create frame carries the client's nonce, so the reply
    /// reaches the CREATING client even when it is not the floating window
    /// (the old `send_app_frame` ack path sent DESKTOP create-acks to the
    /// floating channel — None for the first app → the greeter app-host hung
    /// in `recv_ack` forever and its event loop never armed: dead buttons).
    pub(crate) fn send_frame_for_nonce(&mut self, nonce: u64, frame: &[u8]) -> bool {
        #[cfg(nexus_env = "os")]
        {
            let Some(slot) = self.event_channel_for(nonce) else { return false };
            // The creating client waits in `recv_ack` on this frame: an ack, ordered, kept.
            if self.client_send(slot, frame, Delivery::Ack).accepted() {
                return true;
            }
            let _ = debug_println("WINDOWD: FAIL nonce event send");
            // Report the truth: the client is now waiting for a reply that
            // never arrived, and the caller has to be able to see that.
            false
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (nonce, frame);
            false
        }
    }

    /// Sends a frame on the channel of the surface's OWNER — the PRESENT/
    /// DESTROY-ack route: the desktop surface has its own channel; everything
    /// else is the floating window's.
    pub(crate) fn send_surface_frame(&mut self, surface_id: u32, frame: &[u8]) -> bool {
        #[cfg(nexus_env = "os")]
        {
            if self.desktop_surface_id == Some(surface_id) {
                let Some(slot) = self.desktop_channel else { return false };
                // PRESENT/DESTROY ack — the client waits on it. Dropping one stalled the
                // client until its own timeout (the 504 ms `STALL present stuck` of
                // 2026-07-26): an ack is ordered and kept while the client lives.
                if self.client_send(slot, frame, Delivery::Ack).accepted() {
                    return true;
                }
                let _ = debug_println("WINDOWD: FAIL desktop event send");
                return false;
            }
            let Some(idx) = self.app_index_by_surface(surface_id) else { return false };
            self.send_app_class(idx, frame, Delivery::Ack)
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (surface_id, frame);
            false
        }
    }

    /// Invalidate the CACHED glass backdrop of every visible app window
    /// (except `skip`) whose frame intersects `rect`. The cache is a
    /// snapshot of what was BEHIND the window when it was built — content
    /// changing underneath (another window presenting, the desktop
    /// repainting, a drag) leaves it stale, which showed as "blur shows the
    /// wallpaper, not the actual background". Over-invalidation is safe:
    /// the next present just re-blurs that region live.
    pub(crate) fn invalidate_blur_over(&mut self, rect: DamageRect, skip: Option<usize>) {
        for i in 0..self.apps.len() {
            if Some(i) == skip || !self.apps[i].win.visible || self.apps[i].win.blur_valid == false
            {
                continue;
            }
            let f = self.app_window_rect(i);
            let x0 = rect.x.max(f.x);
            let y0 = rect.y.max(f.y);
            let x1 = (rect.x + rect.width).min(f.x + f.width);
            let y1 = (rect.y + rect.height).min(f.y + f.height);
            if x1 > x0 && y1 > y0 {
                self.apps[i].win.blur_valid = false;
            }
        }
    }

    /// `SURFACE_PRESENT` (v2): the slot the client drew is what the frames read from now on;
    /// the screen follows the named rects where they show — a base row at the surface's place,
    /// a band's rows in its scroll layer's viewport. The ack is the release (RFC-0114 §5), sent
    /// by `release_presents` once every other slot is free — at once when it already is.
    /// `Some`: refused, the NACK to send now.
    /// `OP_SURFACE_PRESENT` from `sender_sid`. `Some((nack, owner_sees))`: refused — told the
    /// surface's owner only when the sender IS the owner (a present naming another app's
    /// surface, a dead compositor's id reused by a new one, is refused to its sender alone).
    pub(crate) fn handle_surface_present(
        &mut self,
        frame: &[u8],
        sender_sid: u64,
    ) -> Option<([u8; wire::SURFACE_ACK_FRAME_LEN], bool)> {
        use super::surface_buffers::Owner;
        let Some(p) = wire::decode_surface_present(frame) else {
            let malformed = wire::SURFACE_STATUS_MALFORMED;
            return Some((wire::encode_surface_ack(wire::OP_SURFACE_PRESENT, malformed, 0), false));
        };
        let (surface_id, seq) = (p.surface_id, p.seq);
        if !self.presents_into(surface_id, sender_sid) {
            // A surface that is gone (its client's late present — a greeter the shell replaced)
            // is stale; one that is another's is a bug or an attack: named, refused alike.
            let gone = self.client_surfaces.get_by_id(surface_id).is_none();
            if self.app_present_reject_markers < 8 {
                self.app_present_reject_markers += 1;
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: {} id={surface_id} seq={seq} sender={sender_sid:#x}",
                    if gone {
                        "surface present stale (gone)"
                    } else {
                        "FAIL surface present denied"
                    }
                ));
            }
            let status =
                if gone { wire::SURFACE_STATUS_BAD_SURFACE } else { wire::SURFACE_STATUS_DENIED };
            return Some((wire::encode_surface_ack(wire::OP_SURFACE_PRESENT, status, seq), false));
        }
        let now = nexus_abi::nsec().unwrap_or(0);
        match self.client_surfaces.present(surface_id, (seq, p.slot), p.damage(), now) {
            Ok((surface, rects, count)) => {
                let rects = &rects[..count];
                match self.owner_of(surface_id) {
                    Some(Owner::Desktop) => {
                        let screen = (0, 0, self.mode.width, self.mode.height);
                        let change = self.desktop_layers.activate();
                        self.present_damage(&surface, rects, change, None, screen);
                        // The desktop it replaced (a re-create) goes with this one's frame.
                        self.end_retiring(Owner::Desktop);
                        // The session's first desktop frame completes the reveal: the revealed
                        // frame is built now — with this present's structure in effect.
                        self.note_desktop_content();
                    }
                    Some(Owner::App(idx)) => {
                        let rect = self.app_window_rect(idx);
                        let screen = (rect.x, rect.y, rect.width, rect.height);
                        let change = self.apps[idx].layers.activate();
                        self.present_damage(&surface, rects, change, Some(idx), screen);
                        // The app's frame takes its window over: the starting surface and a
                        // surface it replaces give way in this present (TASK-0336, RFC-0114).
                        if self.end_starting_surface(idx) || self.apps[idx].retiring.is_some() {
                            self.end_retiring(Owner::App(idx));
                            self.queue_dirty_rect(rect);
                        }
                        // Glass windows above see it through their backdrop: their blur is stale.
                        self.invalidate_blur_over(rect, Some(idx));
                    }
                    None => {}
                }
                // A surface's FIRST present proves the chain is live — once per surface: later
                // presents follow keystrokes too (the keystroke privacy rule).
                if seq == 1 {
                    let _ = debug_println(&alloc::format!(
                        "WINDOWD: surface presented id={surface_id} seq={seq} slot={} t_us={}",
                        p.slot,
                        nexus_abi::nsec().unwrap_or(0) / 1000
                    ));
                }
                self.release_presents();
                None
            }
            Err(status) => {
                // A rejected present is otherwise silent — name it (bounded). A surface its client
                // or its successor replaced (the greeter's last present at the shell's start) is
                // stale, not a fault.
                let stale = self.client_surfaces.get_by_id(surface_id).is_some_and(|s| s.retiring);
                if self.app_present_reject_markers < 8 {
                    self.app_present_reject_markers += 1;
                    let _ = debug_println(&alloc::format!(
                        "WINDOWD: {}surface present {} id={surface_id} seq={seq} slot={} status={status}",
                        if stale { "" } else { "FAIL " },
                        if stale { "stale (replaced)" } else { "rejected" },
                        p.slot
                    ));
                }
                Some((wire::encode_surface_ack(wire::OP_SURFACE_PRESENT, status, seq), true))
            }
        }
    }

    /// `SURFACE_DESTROY`: the client drops its surface — a re-create follows (a resize, a mode
    /// switch, a store that outgrew it). The surface takes no present any more and shows its
    /// last frame until its successor presents (RFC-0114); the window keeps its place and mode
    /// (hiding it would clear its fullscreen flag — the "fullscreen makes everything vanish"
    /// bug). A user's close is `close_app_window`, not this.
    pub(crate) fn handle_surface_destroy(
        &mut self,
        frame: &[u8],
        sender_owns: bool,
    ) -> [u8; wire::SURFACE_ACK_FRAME_LEN] {
        use super::surface_buffers::Owner;
        let Some(surface_id) = wire::decode_surface_destroy(frame) else {
            let malformed = wire::SURFACE_STATUS_MALFORMED;
            return wire::encode_surface_ack(wire::OP_SURFACE_DESTROY, malformed, 0);
        };
        if !sender_owns {
            let denied = wire::SURFACE_STATUS_DENIED;
            return wire::encode_surface_ack(wire::OP_SURFACE_DESTROY, denied, surface_id);
        }
        let owner = if self.desktop_surface_id == Some(surface_id) {
            Owner::Desktop
        } else if let Some(idx) = self.app_index_by_surface(surface_id) {
            Owner::App(idx)
        } else {
            let bad = wire::SURFACE_STATUS_BAD_SURFACE;
            return wire::encode_surface_ack(wire::OP_SURFACE_DESTROY, bad, surface_id);
        };
        // A dying surface takes its text focus with it — and the OSK band must follow
        // (RFC-0075: never a keyboard without a field).
        if self.text_focus.map(|f| f.surface_id) == Some(surface_id) {
            self.text_focus = None;
            self.update_osk_visibility();
        }
        match owner {
            Owner::Desktop => self.desktop_surface_id = None,
            Owner::App(idx) => self.apps[idx].surface_id = None,
        }
        self.retire_surface(owner, surface_id);
        let _ = debug_println(&alloc::format!("WINDOWD: surface destroyed id={surface_id}"));
        wire::encode_surface_ack(wire::OP_SURFACE_DESTROY, wire::SURFACE_STATUS_OK, surface_id)
    }

    /// The app surface's resolved presentation — declared intent ⟂ the
    /// environment's windowing policy, via the ONE host-tested resolver
    /// (`surface_presentation`). All compositing decisions (chrome, z-band,
    /// full-screen, resize) read THIS; nothing re-derives from raw intent tags.
    pub(super) fn app_presentation(
        &self,
        idx: usize,
    ) -> crate::surface_presentation::WindowPresentation {
        crate::surface_presentation::WindowPresentation::resolve(
            self.apps[idx].intent_style,
            self.apps[idx].intent_level,
            // WM override (app-menu mode switch) wins over the declared
            // intent; a fresh launch cleared it back to the intent.
            self.apps[idx].wm_mode.unwrap_or(self.apps[idx].intent_mode),
            self.apps[idx].intent_resizable,
            self.windowing_policy,
        )
    }

    /// Stores the app's dedicated event channel (SEND cap slot moved with an
    /// `OP_SURFACE_EVENTS` frame, execd-attached). A relaunch replaces the
    /// channel — the previous cap is closed, never leaked.
    #[allow(unused_variables)]
    /// The event channel bound to `nonce`, if attached. Non-consuming — a
    /// resize RE-create binds the same nonce again.
    #[cfg(nexus_env = "os")]
    pub(super) fn event_channel_for(&self, nonce: u64) -> Option<u32> {
        self.event_channels.iter().find(|e| e.0 == nonce).map(|e| e.1)
    }

    pub(crate) fn attach_app_event_channel(&mut self, send_slot: Option<u32>, nonce: Option<u64>) {
        #[cfg(not(nexus_env = "os"))]
        let _ = nonce;
        #[cfg(nexus_env = "os")]
        {
            let Some(slot) = send_slot else {
                let _ = debug_println("WINDOWD: FAIL app event channel (no cap)");
                return;
            };
            let Some(nonce) = nonce else {
                let _ = debug_println("WINDOWD: FAIL app event channel (no nonce)");
                let _ = nexus_abi_cap_close(slot);
                return;
            };
            // Bind nonce → channel (a rebind of the same nonce replaces its channel). The table
            // grows: a live channel is never evicted (RFC-0114 §7 — the ninth app's attach used
            // to evict a LIVE app's channel, and that app saw EOF and exited). Every (re)bind
            // tells the snapshot tracker (RFC-0083): a fresh channel owes the current
            // presentation and forgives failures.
            if let Some(i) = self.event_channels.iter().position(|e| e.0 == nonce) {
                // The replaced channel's waiting frames go with it (ADR-0075).
                self.forget_client_channel(self.event_channels[i].1);
                let _ = nexus_abi_cap_close(self.event_channels[i].1);
                self.event_channels[i].1 = slot;
                self.presentation.note_bound(i);
            } else if self.event_channels.try_reserve(1).is_ok() {
                self.event_channels.push((nonce, slot));
                self.presentation.note_bound(self.event_channels.len() - 1);
            } else {
                let _ = debug_println("WINDOWD: FAIL app event channel (no memory)");
                let _ = nexus_abi_cap_close(slot);
                return;
            }
            // Nonce logged: desktop-bind-race triage matches attach vs deferred bind.
            let _ = debug_println(&alloc::format!(
                "WINDOWD: app event channel attached nonce={nonce:#x}"
            ));
            // The presentation snapshot (theme+profile+region, RFC-0083)
            // reaches this channel via `pump_presentation` — `note_bound`
            // above marked it due, the next loop iteration delivers it, and
            // the app's boot wait consumes it before mounting.
            // Complete a desktop bind that raced ahead of this attach (if any).
            self.complete_deferred_desktop_bind(nonce, slot);
            // A buffer whose create raced this attach goes to its client now (RFC-0114).
            self.deliver_owed_buffers();
            // Flush a PARKED intent reply for this nonce (boot-park track):
            // the geometry answer the racing intent could not deliver.
            while let Some(i) = self.pending_intent_replies.iter().position(|(n, _)| *n == nonce) {
                let (_, rect) = self.pending_intent_replies.swap_remove(i);
                let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, rect.len() as u32);
                let _ = nexus_abi::ipc_send_v1(slot, &hdr, &rect, nexus_abi::IPC_SYS_NONBLOCK, 0);
                let _ = debug_println("WINDOWD: parked intent reply flushed");
            }
        }
    }

    /// Active shell profile as its wire tag (from the SystemUI shell config SSOT).
    pub(crate) fn shell_profile_wire(&self) -> u8 {
        use nexus_display_proto::client_surface as wire;
        match self.shell_config.profile_id.as_str() {
            "tablet" => wire::PROFILE_TABLET,
            "phone" => wire::PROFILE_PHONE,
            "tv" => wire::PROFILE_TV,
            _ => wire::PROFILE_DESKTOP,
        }
    }

    /// Sends the active shell profile to EVERY connected app-host (floating
    /// window, desktop shell, pending channels) — the live Desktop/Tablet
    /// switch; apps re-mount so their platform override arms re-select.
    /// Sends the active theme mode to the app on its event channel (`chrome =
    /// intent ⟂ policy` for colours too: the WM owns the theme, apps follow).
    /// The packed theme byte for `OP_SURFACE_THEME` pushes: low nibble =
    /// mode, high nibble = the accent-palette index (`pack_theme`).
    pub(crate) fn theme_wire_byte(&self) -> u8 {
        use nexus_display_proto::client_surface as wire;
        let mode = match self.theme_mode {
            crate::theme::ThemeMode::Dark => wire::THEME_DARK,
            crate::theme::ThemeMode::Light => wire::THEME_LIGHT,
        };
        let solid = if self.solid_materials { wire::THEME_SOLID } else { 0 };
        wire::pack_theme(mode, self.theme_accent) | solid
    }

    /// Sends one app-bound frame of USER INTENT (a text commit, a key, a tap, a window rect)
    /// on the dedicated event channel. Returns false when no channel is attached (the caller
    /// falls back to the shared response endpoint) or the frame could not be accepted. Intent
    /// has no recovery path: it is ordered and never dropped while the client lives — and never
    /// parks the compositor (ADR-0075): a busy client's frames wait in its outbox. (A first cut
    /// dropped intent on a full queue — the clipboard's commit and the screenshot's shutter went
    /// missing, TASK-0330 S2a; the next one parked windowd on the queue — a launching app froze
    /// every window, cycle 54.)
    #[allow(unused_variables)]
    pub(crate) fn send_app_frame(&mut self, idx: usize, frame: &[u8]) -> bool {
        self.send_app_class(idx, frame, Delivery::Intent)
    }

    #[allow(unused_variables)]
    fn send_app_class(&mut self, idx: usize, frame: &[u8], delivery: Delivery) -> bool {
        #[cfg(nexus_env = "os")]
        {
            use super::client_send::ClientSend;
            let Some(slot) = self.apps[idx].event_channel else { return false };
            match self.client_send(slot, frame, delivery) {
                ClientSend::Sent | ClientSend::Queued => return true,
                // TASK-0333 6a: the peer is gone — the window closes by name.
                ClientSend::Dead => self.note_app_channel_dead(idx),
                ClientSend::Refused => {
                    let _ = debug_println("WINDOWD: FAIL app event send");
                }
            }
            false
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (frame, delivery);
            false
        }
    }

    /// Routes a wheel notch to an app window (`INPUT_KIND_WHEEL`): the signed delta rides the wire
    /// `y` field UNCLAMPED (`wheel_delta_to_wire` — the pointer kinds' `max(0)` clamp would
    /// destroy negative deltas), ordered through the client's outbox.
    pub(crate) fn send_app_wheel(&mut self, idx: usize, local_x: u16, wire_delta: u16) {
        #[cfg(nexus_env = "os")]
        {
            let Some(client) =
                self.apps[idx].surface_id.and_then(|id| self.client_surfaces.get_by_id(id))
            else {
                return;
            };
            let frame = nexus_display_proto::client_surface::encode_surface_input(
                client.id,
                nexus_display_proto::client_surface::INPUT_KIND_WHEEL,
                local_x,
                wire_delta,
            );
            if let Some(slot) = self.apps[idx].event_channel {
                let kind = nexus_display_proto::client_surface::INPUT_KIND_WHEEL;
                let _ = self.client_send(slot, &frame, Delivery::for_input(kind));
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (idx, local_x, wire_delta);
        }
    }

    /// Routes a body tap to the surface's owning app (R3) on its DEDICATED event channel (the
    /// shared response endpoint raced with inputd's ack drain), with the press's facts.
    pub(crate) fn send_app_input(&mut self, idx: usize, x: i32, y: i32) {
        self.send_app_input_kind(idx, nexus_display_proto::client_surface::INPUT_KIND_TAP, x, y);
    }

    /// `send_app_input` for any input kind (`true` = it went out). Taps keep
    /// their routed/FAIL markers; MOVE/LEAVE/DRAG are frame-rate traffic, silent.
    pub(crate) fn send_app_input_kind(&mut self, idx: usize, kind: u8, x: i32, y: i32) -> bool {
        #[cfg(nexus_env = "os")]
        {
            let is_tap = kind == nexus_display_proto::client_surface::INPUT_KIND_TAP;
            let Some(client) =
                self.apps[idx].surface_id.and_then(|id| self.client_surfaces.get_by_id(id))
            else {
                return false;
            };
            // A tap carries the press's facts, a MOVE or DRAG its samples (`motion.rs`).
            let (frame, n) = self.pointer_frame(client.id, kind, x, y);
            let Some(slot) = self.apps[idx].event_channel else {
                if is_tap {
                    let _ = debug_println("WINDOWD: FAIL surface input (no event channel)");
                }
                return false;
            };
            let sent = self.client_send(slot, &frame[..n], Delivery::for_input(kind)).accepted();
            if sent && is_tap && super::input_hover::first_tap_routed(false) {
                let _ = debug_println("WINDOWD: surface input routed");
            } else if !sent && is_tap {
                let _ = debug_println("WINDOWD: FAIL surface input send");
            }
            sent
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (idx, kind, x, y);
            false
        }
    }
}

/// Thin cap-close shim so the handlers above read cleanly on host builds
/// (where `cap_close` does not exist).
#[cfg(nexus_env = "os")]
pub(super) fn nexus_abi_cap_close(slot: u32) -> core::result::Result<(), ()> {
    nexus_abi::cap_close(slot).map_err(|_| ())
}

#[cfg(not(nexus_env = "os"))]
pub(super) fn nexus_abi_cap_close(_slot: u32) -> core::result::Result<(), ()> {
    Ok(())
}
