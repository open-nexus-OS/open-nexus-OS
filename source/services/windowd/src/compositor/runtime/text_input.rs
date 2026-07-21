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
//! TEST_COVERAGE: host tests in this module (sender resolution + routing
//! bookkeeping); the wire path is proven by the QEMU IME selftests.

use super::*;
use nexus_display_proto::surface_text;
use nexus_wire::imed as ime_wire;

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
    /// `OP_SURFACE_TEXT_FOCUS` from an app: resolve the SENDER's surface by
    /// kernel identity (never by payload), record the route, relay to imed.
    pub(crate) fn handle_surface_text_focus(&mut self, frame: &[u8], sender_sid: u64) {
        let Some((focused, field_kind, caret)) = surface_text::decode_surface_text_focus(frame)
        else {
            let _ = debug_println("WINDOWD: FAIL text-focus (malformed)");
            return;
        };
        let Some((target, surface_id)) = self.text_sender_surface(sender_sid) else {
            // Unknown sender: no surface of this identity — fail closed.
            return;
        };
        if focused {
            self.text_focus = Some(TextFocusRoute { target, surface_id, field_kind });
        } else {
            // Only the current holder can clear (a stale unfocus from a
            // background surface must not drop the active field's focus).
            if self.text_focus.map(|f| f.surface_id) == Some(surface_id) {
                self.text_focus = None;
            } else {
                return;
            }
        }
        self.relay_focus_to_imed(u64::from(surface_id), focused, field_kind, caret);
    }

    /// imed push (`'I','E'` frames): route commit/action to the focused
    /// surface's app channel. Sender-gated to the IME authority.
    pub(crate) fn handle_imed_push(&mut self, frame: &[u8], sender_sid: u64) {
        if sender_sid != nexus_abi::service_id_from_name(b"imed") {
            let _ = debug_println("WINDOWD: FAIL imed push (foreign sender)");
            return;
        }
        let (surface_id, kind, aux, buf, len);
        if let Some((sid, text)) = ime_wire::decode_commit(frame) {
            surface_id = sid;
            kind = surface_text::SURFACE_TEXT_COMMIT;
            aux = 0;
            let Some((b, n)) = surface_text::encode_surface_text(kind, aux, text) else {
                return;
            };
            buf = b;
            len = n;
        } else if let Some((sid, act)) = ime_wire::decode_action(frame) {
            surface_id = sid;
            kind = surface_text::SURFACE_TEXT_ACTION;
            aux = act;
            let Some((b, n)) = surface_text::encode_surface_text(kind, aux, "") else {
                return;
            };
            buf = b;
            len = n;
        } else if let Some((sid, caret, text)) = ime_wire::decode_preedit(frame) {
            surface_id = sid;
            kind = surface_text::SURFACE_TEXT_PREEDIT;
            aux = caret;
            let Some((b, n)) = surface_text::encode_surface_text(kind, aux, text) else {
                return;
            };
            buf = b;
            len = n;
        } else {
            return; // candidates land with RFC-0075 Phase 3; unknown = drop
        }
        // Route ONLY to the recorded focus holder — a surface id in the push
        // that doesn't match the focus route is dropped (stale focus race).
        let Some(route) = self.text_focus else {
            return;
        };
        if u64::from(route.surface_id) != surface_id {
            return;
        }
        match route.target {
            TextFocusTarget::App(idx) => {
                let _ = self.send_app_frame(idx, &buf[..len]);
            }
            TextFocusTarget::Desktop => {
                self.send_desktop_frame(&buf[..len]);
            }
        }
    }

    /// Resolves the sender's surface: floating app slot by `owner_sid`, else
    /// the desktop surface. `None` for identities that own no surface.
    fn text_sender_surface(&self, sender_sid: u64) -> Option<(TextFocusTarget, u32)> {
        if let Some(idx) =
            self.apps.iter().position(|a| a.owner_sid == sender_sid && a.surface_id.is_some())
        {
            return self.apps[idx].surface_id.map(|sid| (TextFocusTarget::App(idx), sid));
        }
        if sender_sid != 0 && sender_sid == self.desktop_owner_sid {
            return self.desktop_surface_id.map(|sid| (TextFocusTarget::Desktop, sid));
        }
        None
    }

    /// Fire-and-forget frame to the desktop surface's event channel.
    #[allow(unused_variables)]
    fn send_desktop_frame(&mut self, frame: &[u8]) {
        #[cfg(nexus_env = "os")]
        if let Some(slot) = self.desktop_channel {
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
            if nexus_abi::ipc_send_v1(slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0).is_err() {
                let _ = debug_println("WINDOWD: FAIL desktop text send");
            }
        }
    }

    /// Fire-and-forget `OP_SET_FOCUS` relay to imed (lazy route; the reply,
    /// if any, lands on the shared inbox and is ignored).
    #[allow(unused_variables)]
    fn relay_focus_to_imed(
        &mut self,
        surface_id: u64,
        focused: bool,
        field_kind: u8,
        caret: surface_text::CaretRect,
    ) {
        #[cfg(nexus_env = "os")]
        {
            if self.imed_client.is_none() {
                self.imed_client = nexus_ipc::KernelClient::new_for("imed").ok();
            }
            let Some(client) = self.imed_client.as_ref() else {
                let _ = debug_println("WINDOWD: FAIL imed route");
                return;
            };
            let frame = ime_wire::encode_set_focus(
                surface_id,
                u8::from(focused),
                field_kind,
                caret.0,
                caret.1,
                caret.2,
                caret.3,
            );
            use nexus_ipc::Client as _;
            if client.send(&frame, Wait::NonBlocking).is_err() {
                // Drop the cached route; the next transition re-resolves.
                self.imed_client = None;
            }
        }
    }
}
