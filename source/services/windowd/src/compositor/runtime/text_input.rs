// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — RFC-0075 text routing: apps announce
//! widget focus (`OP_SURFACE_TEXT_FOCUS`), windowd relays it to imed
//! (`OP_SET_FOCUS`) and routes imed's commit/action pushes back to the
//! focused surface (`OP_SURFACE_TEXT`). PURE ROUTING — no IME state machine,
//! no text interpretation, no UI (compositor boundary).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the push gate in `key_gate` on the host (`tests/key_gate.rs`); the wire path
//! is proven by the QEMU IME selftests.

use super::*;
use nexus_display_proto::surface_text;
use nexus_wire::imed as ime_wire;

use crate::client_surface::Delivery;
use crate::key_gate;

/// The focused surface's routing entry (identity-derived, RFC-0075).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextFocusRoute {
    pub(crate) target: TextFocusTarget,
    pub(crate) surface_id: u32,
    pub(crate) field_kind: u8,
}

/// Where imed output for the focused surface is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextFocusTarget {
    /// Floating app window slot (`apps[idx]` event channel).
    App(usize),
    /// The desktop surface (shell/greeter app-host).
    Desktop,
}

impl DisplayServerRuntime {
    /// `OP_SURFACE_TEXT_FOCUS` from an app: the app CLAIMS its own surface
    /// (windowd's server endpoint carries no per-sender identity for app
    /// processes — sender_sid arrives as 0; same trust level and same
    /// recorded follow-up as `OP_SURFACE_CONTROL`). Blast radius is focus
    /// misdirection only: imed output always routes to the CLAIMED surface's
    /// own event channel, never to the announcer.
    pub(crate) fn handle_surface_text_focus(&mut self, frame: &[u8], _sender_sid: u64) {
        let Some((surface_id, focused, field_kind, caret)) =
            surface_text::decode_surface_text_focus(frame)
        else {
            let _ = debug_println("WINDOWD: FAIL text-focus (malformed)");
            return;
        };
        let target = if self.desktop_surface_id == Some(surface_id) {
            TextFocusTarget::Desktop
        } else if let Some(idx) = self.app_index_by_surface(surface_id) {
            TextFocusTarget::App(idx)
        } else {
            // Unknown surface — fail closed (stale claim after destroy).
            let _ = debug_println("WINDOWD: FAIL text-focus (unknown surface)");
            return;
        };
        if focused {
            self.text_focus = Some(TextFocusRoute { target, surface_id, field_kind });
            // An explicit focus announce (field tap) re-opens a dismissed OSK.
            self.osk_dismissed = false;
        } else {
            // Only the current holder can clear (a stale unfocus from a
            // background surface must not drop the active field's focus).
            if self.text_focus.map(|f| f.surface_id) == Some(surface_id) {
                self.text_focus = None;
            } else {
                return;
            }
        }
        // imed hears a field's focus only where it is in use (RFC-0113 §3): the on-screen
        // keyboard's profiles, or an open composition session (a layout source never opens one).
        if self.imed_in_use() {
            self.relay_focus_to_imed(u64::from(surface_id), focused, field_kind, caret);
            if self.key_route.imed_session.is_some() {
                self.key_route.imed_session = focused.then_some((surface_id, field_kind));
            }
        }
        self.update_osk_visibility();
        // A text blur hands imed's focus back to the WINDOW (ESC still reaches it).
        self.sync_surface_focus_to_imed();
    }

    /// Whether imed is in use: an on-screen-keyboard profile, or a composition session open.
    fn imed_in_use(&self) -> bool {
        use nexus_display_proto::client_surface as wire;
        self.shell_profile_wire() != wire::PROFILE_DESKTOP || self.key_route.imed_session.is_some()
    }

    /// TASK-0074 D3: imed delivers keys ONLY to a focused surface. Without a
    /// text field the focused WINDOW still has to hear ESC (the modal's
    /// dismissal), so windowd tells imed which surface holds window focus with
    /// `FIELD_KIND_NONE` — imed then passes Escape and nothing else (no
    /// commits, no strip, no OSK). A text-field focus REPLACES it (the text
    /// relay above owns imed's focus while a field is focused), a text blur
    /// hands it back. One relay per change; called after input, focus and
    /// window-set changes.
    pub(crate) fn sync_surface_focus_to_imed(&mut self) {
        // Only the on-screen keyboard's profiles keep imed told of the window (RFC-0113 §3: on
        // the desktop a layout's Escape reaches the window directly, a composition session is
        // opened by its first key).
        if self.shell_profile_wire() == nexus_display_proto::client_surface::PROFILE_DESKTOP {
            self.key_route.surface_focus = None;
            return;
        }
        let want = if self.text_focus.is_some() { None } else { self.window_focus_surface() };
        if want == self.key_route.surface_focus {
            return;
        }
        if self.text_focus.is_some() {
            // The text relay already replaced imed's focus — nothing to send.
            self.key_route.surface_focus = None;
            return;
        }
        match (want, self.key_route.surface_focus) {
            (Some(sid), _) => {
                self.relay_focus_to_imed(
                    u64::from(sid),
                    true,
                    ime_wire::FIELD_KIND_NONE,
                    (0, 0, 0, 0),
                );
            }
            (None, Some(prev)) => {
                self.relay_focus_to_imed(
                    u64::from(prev),
                    false,
                    ime_wire::FIELD_KIND_NONE,
                    (0, 0, 0, 0),
                );
            }
            (None, None) => {}
        }
        self.key_route.surface_focus = want;
    }

    /// OSK show/hide (RFC-0075 Phase 8c policy): text focus shows the
    /// on-screen keyboard in TOUCH profiles only (desktop layout = hardware
    /// keyboard flow — user decision) and never while the user dismissed it
    /// (its X; the next field tap re-opens). windowd only composites — the
    /// OSK is the `ime-ui` overlay app, lazily launched on the first focus
    /// (abilitymgr owns the spawn).
    pub(crate) fn update_osk_visibility(&mut self) {
        use nexus_display_proto::client_surface as wire;
        let want_osk = self.text_focus.is_some()
            && self.shell_profile_wire() != wire::PROFILE_DESKTOP
            && !self.osk_dismissed
            // The OSK never targets ITSELF (its own taps must not re-anchor it).
            && !self
                .text_focus
                .and_then(|f| match f.target {
                    TextFocusTarget::App(idx) => Some(idx),
                    TextFocusTarget::Desktop => None,
                })
                .is_some_and(|idx| self.app_is_overlay(idx));
        let osk_idx = (0..self.apps.len())
            .find(|&i| self.apps[i].surface_id.is_some() && self.app_is_overlay(i));
        match (want_osk, osk_idx) {
            (true, Some(idx)) => {
                let wid = crate::window_scene::WindowId::App(idx as u16);
                if !self.windows.is_visible(wid) {
                    self.apps[idx].win.visible = true;
                    self.windows.show_unfocused(wid);
                    let rect = self.app_window_rect(idx);
                    self.queue_dirty_rect(rect);
                }
            }
            (true, None) => {
                // Lazily launch the ime-ui overlay ONCE per boot; its create
                // lands as a normal `OP_SURFACE_CREATE` with `level: overlay`.
                if !self.osk_launch_requested {
                    self.osk_launch_requested = true;
                    self.launch_app("ime-ui");
                }
            }
            (false, Some(idx)) => {
                let wid = crate::window_scene::WindowId::App(idx as u16);
                if self.windows.is_visible(wid) {
                    let rect = self.app_window_rect(idx);
                    self.apps[idx].win.visible = false;
                    self.windows.hide(wid);
                    self.queue_dirty_rect(rect);
                }
            }
            (false, None) => {}
        }
    }

    /// `OP_SURFACE_CURSOR_HINT` from an app: the desired pointer shape while
    /// the pointer hovers that surface's body (I-beam over editable fields).
    /// Data-only; the shape re-resolves immediately so a hint under a static
    /// pointer takes effect without a pointer move.
    pub(crate) fn handle_surface_cursor_hint(&mut self, frame: &[u8]) {
        let Some((surface_id, shape)) = surface_text::decode_surface_cursor_hint(frame) else {
            return;
        };
        if self.desktop_surface_id == Some(surface_id) {
            self.desktop_cursor_hint = shape;
        } else if let Some(idx) =
            (0..self.apps.len()).find(|&i| self.apps[i].surface_id == Some(surface_id))
        {
            self.apps[idx].cursor_hint = shape;
        } else {
            return;
        }
        self.update_cursor_shape_for_pointer(self.state.cursor_x, self.state.cursor_y);
    }

    /// Whether app slot `idx` declared the OVERLAY level (the OSK band).
    fn app_is_overlay(&self, idx: usize) -> bool {
        self.apps[idx].intent_level == nexus_display_proto::client_surface::WIN_LEVEL_OVERLAY
    }

    /// The OSK overlay's app slot, when its surface is live.
    pub(super) fn osk_idx(&self) -> Option<usize> {
        (0..self.apps.len()).find(|&i| self.apps[i].surface_id.is_some() && self.app_is_overlay(i))
    }

    /// Relays a preedit snapshot to the ime-ui strip (`OP_SURFACE_IME_STATE`).
    fn push_ime_state_to_osk_preedit(&mut self, text: &str) {
        let Some(idx) = self.osk_idx() else { return };
        if let Some((frame, n)) = surface_text::encode_ime_preedit(text) {
            let _ = self.send_app_frame(idx, &frame[..n]);
        }
    }

    /// Relays a candidate page to the ime-ui strip. The imed wire packs
    /// entries as `len:u8, bytes…` — unpack bounded, re-encode for the app.
    fn push_ime_state_to_osk_candidates(&mut self, page: u8, count: u8, list: &[u8]) {
        let Some(idx) = self.osk_idx() else { return };
        let mut texts: [&str; surface_text::IME_CANDIDATES_MAX] =
            [""; surface_text::IME_CANDIDATES_MAX];
        let count = usize::from(count).min(surface_text::IME_CANDIDATES_MAX);
        let mut n = 0usize;
        for slot in texts.iter_mut().take(count) {
            let Some(&len) = list.get(n) else { return };
            n += 1;
            let Some(bytes) = list.get(n..n + usize::from(len)) else { return };
            let Ok(text) = core::str::from_utf8(bytes) else { return };
            *slot = text;
            n += usize::from(len);
        }
        if let Some((frame, fl)) = surface_text::encode_ime_candidates(page, &texts[..count]) {
            let _ = self.send_app_frame(idx, &frame[..fl]);
        }
    }

    /// imed push (`'I','E'` frames): route commit/action to the focused
    /// surface's app channel. Sender-gated to the IME authority.
    pub(crate) fn handle_imed_push(&mut self, frame: &[u8], sender_sid: u64) {
        if key_gate::imed_sender(sender_sid, nexus_abi::service_id_from_name(b"imed")).is_err() {
            let _ = debug_println("WINDOWD: FAIL imed push (foreign sender)");
            return;
        }
        let (surface_id, kind, aux, buf, len);
        if let Some((sid, text)) = ime_wire::decode_commit(frame) {
            surface_id = sid;
            kind = surface_text::SURFACE_TEXT_COMMIT;
            aux = 0;
            let Some((b, n)) =
                surface_text::encode_surface_text(kind, aux, text, surface_text::TextStamp::NONE)
            else {
                return;
            };
            buf = b;
            len = n;
        } else if let Some((sid, act)) = ime_wire::decode_action(frame) {
            surface_id = sid;
            kind = surface_text::SURFACE_TEXT_ACTION;
            aux = act;
            let Some((b, n)) =
                surface_text::encode_surface_text(kind, aux, "", surface_text::TextStamp::NONE)
            else {
                return;
            };
            buf = b;
            len = n;
        } else if let Some((_sid, _caret, text)) = ime_wire::decode_preedit(frame) {
            // Strip state (RFC-0075 Phase 3): preedit previews live in the
            // ime-ui overlay, never inline in the app — relay + done (imed
            // only pushes while a surface holds text focus).
            self.push_ime_state_to_osk_preedit(text);
            return;
        } else if let Some((_sid, page, count, list)) = ime_wire::decode_candidates(frame) {
            self.push_ime_state_to_osk_candidates(page, count, list);
            return;
        } else {
            return; // unknown = drop
        }
        // Route ONLY to the recorded focus holder — a surface id in the push
        // that doesn't match the focus route is dropped (stale focus race).
        // Without a text field, the WINDOW-focus relay (`FIELD_KIND_NONE`,
        // TASK-0074 D3) is the holder — and imed sends it ACTIONS only
        // (Escape, the modal's dismissal); anything else is dropped here too.
        let holders = key_gate::Holders {
            field: self.text_focus.map(|route| route.surface_id),
            window: self.key_route.surface_focus.or(match self.key_route.imed_session {
                Some((sid, ime_wire::FIELD_KIND_NONE)) => Some(sid),
                _ => None,
            }),
        };
        let is_action = kind == surface_text::SURFACE_TEXT_ACTION;
        let Ok(holder) = key_gate::imed_push_holder(surface_id, is_action, holders) else {
            return;
        };
        let target = match self.text_focus {
            Some(route) if route.surface_id == holder => route.target,
            _ if self.desktop_surface_id == Some(holder) => TextFocusTarget::Desktop,
            _ => match self.app_index_by_surface(holder) {
                Some(idx) => TextFocusTarget::App(idx),
                None => return,
            },
        };
        match target {
            TextFocusTarget::App(idx) => {
                let _ = self.send_app_frame(idx, &buf[..len]);
            }
            TextFocusTarget::Desktop => {
                self.send_desktop_frame(&buf[..len]);
            }
        }
    }

    /// A frame to the desktop surface's event channel — user intent (`Delivery::Intent`):
    /// ordered, never dropped while the desktop lives, never a park (ADR-0075); a real failure
    /// is reported.
    #[allow(unused_variables)]
    pub(super) fn send_desktop_frame(&mut self, frame: &[u8]) {
        #[cfg(nexus_env = "os")]
        if let Some(slot) = self.desktop_channel {
            if !self.client_send(slot, frame, Delivery::Intent).accepted() {
                let _ = debug_println("WINDOWD: FAIL desktop text send");
            }
        }
    }

    /// `OP_SET_FOCUS` to imed through the ordered outbox (RFC-0113 §3) — never dropped while imed
    /// lives, never a park; a refused relay is said (the old relay recorded the focus as sent
    /// even when its non-blocking send failed, and never repeated it).
    fn relay_focus_to_imed(
        &mut self,
        surface_id: u64,
        focused: bool,
        field_kind: u8,
        caret: surface_text::CaretRect,
    ) {
        let frame = ime_wire::encode_set_focus(
            surface_id,
            u8::from(focused),
            field_kind,
            caret.0,
            caret.1,
            caret.2,
            caret.3,
        );
        if !self.send_to_imed(&frame) {
            let _ = debug_println("WINDOWD: FAIL imed relay send");
        }
    }
}
