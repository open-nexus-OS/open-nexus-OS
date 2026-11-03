// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd's end of the key channel (RFC-0113 §3, ADR-0090). inputd sends what the keys
//! produced (`OP_PUSH_KEY_INPUT` — a blocking send on its side: a full queue holds inputd, it
//! never drops a key). A layout source's text and editing actions go to the surface that holds
//! WINDOW focus: its app routes them to its focused field in the order its own events arrive, so
//! a key never overtakes the tap that focused the field (the tap's edge is applied while the
//! queue drains, before the key behind it). A composition source's keys go to the input method
//! behind this focus: windowd opens imed's session (`OP_SET_FOCUS`) and sends the keys after it
//! on ONE ordered outbox (ADR-0075) — focus and keys travel one path, in one order. imed hears
//! nothing while the source is a layout and no on-screen keyboard is in use. A key with nobody to
//! take it (no window yet, a window without its channel) is counted and said once; a key a
//! receiver could not be handed is LOST — counted and named (`WINDOWD: FAIL key input`). The
//! system's own keys arrive here too (TASK-0348 P17): a window chord is applied to the focused
//! window and a capture key handed to the shell — once each, in order, never a fact that a
//! coalesced sample could drop (chain hop K4 says the chord arrived).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the decisions in `key_gate` on the host (`tests/key_gate.rs`); the QEMU typing
//!   scenario (`apphost: input session (`) end to end

use super::*;
use input_live_protocol::{
    decode_push_key_input, KeyInput, KEY_INPUT_ACTION, KEY_INPUT_CAPTURE, KEY_INPUT_CHORD,
    KEY_INPUT_COMPOSE, KEY_INPUT_TEXT, MAX_KEY_INPUTS,
};
use nexus_display_proto::surface_text::{self, TextStamp};
use nexus_wire::imed as ime_wire;

use crate::client_surface::Delivery;
use crate::key_gate::{self, SessionStep, SystemKey};

/// Failure lines a boot prints before it only counts (a key path that keeps failing is named by
/// its first lines and its counter, not by a line per key).
const FAIL_LINES_MAX: u32 = 8;

/// What became of a key windowd handed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handed {
    /// In its receiver's queue or ordered outbox.
    Delivered,
    /// Nobody to take it: no focused window, a window without its channel, a receiver gone.
    NoReceiver,
    /// A living receiver could not be handed it.
    Lost,
}

/// The key route's state: imed's session and window-focus relay, the counters.
#[derive(Debug, Default)]
pub(crate) struct KeyRoute {
    /// The session imed holds — `(surface, field kind)`; `None` = none open.
    pub(super) imed_session: Option<(u32, u8)>,
    /// imed's WINDOW-focus relay (`FIELD_KIND_NONE`, TASK-0074 D3) while no field holds it —
    /// kept only where imed is in use (the on-screen keyboard's profiles).
    pub(super) surface_focus: Option<u32>,
    /// Keys a living receiver could not be handed.
    lost: u32,
    /// Keys nobody could take.
    unreceived: u32,
    fail_lines: u32,
    stranger_said: bool,
}

impl KeyRoute {
    pub(crate) const fn new() -> Self {
        Self {
            imed_session: None,
            surface_focus: None,
            lost: 0,
            unreceived: 0,
            fail_lines: 0,
            stranger_said: false,
        }
    }
}

impl DisplayServerRuntime {
    /// `OP_PUSH_KEY_INPUT` from inputd: every entry delivered in order, or counted.
    pub(crate) fn handle_key_input(&mut self, frame: &[u8], sender_sid: u64) {
        if key_gate::key_sender(sender_sid, nexus_abi::service_id_from_name(b"inputd")).is_err() {
            if !core::mem::replace(&mut self.key_route.stranger_said, true) {
                let _ = debug_println("WINDOWD: FAIL key input (foreign sender)");
            }
            return;
        }
        let mut inputs = [KeyInput::default(); MAX_KEY_INPUTS];
        let Some(n) = decode_push_key_input(frame, &mut inputs) else {
            self.note_key_lost("malformed");
            return;
        };
        for input in &inputs[..n] {
            let handed = match input.kind {
                KEY_INPUT_TEXT | KEY_INPUT_ACTION => self.deliver_key(input),
                KEY_INPUT_COMPOSE => self.compose_key(input),
                KEY_INPUT_CHORD | KEY_INPUT_CAPTURE => self.system_key(input),
                _ => Handed::Lost,
            };
            match handed {
                Handed::Delivered => {}
                Handed::NoReceiver => self.note_key_unreceived(),
                Handed::Lost => self.note_key_lost("undeliverable"),
            }
        }
    }

    /// The system's own key (TASK-0348 P17): a window chord tiles, minimizes, closes or restarts
    /// (`apply_chord`), a capture key goes to the shell — needs no focused field, applied once.
    fn system_key(&mut self, input: &KeyInput) -> Handed {
        match key_gate::system_key(input.kind, input.value) {
            Some(SystemKey::Chord(code)) => self.apply_chord(code),
            Some(SystemKey::Capture(kind)) => self.push_capture_key(kind),
            None => return Handed::Lost,
        }
        Handed::Delivered
    }

    /// A layout source's text or action to the window-focused surface, stamped with its key.
    fn deliver_key(&mut self, input: &KeyInput) -> Handed {
        let stamp = TextStamp { seq: input.seq, t_ns: input.t_ns };
        let mut utf8 = [0u8; 4];
        let (kind, aux, text) = if input.kind == KEY_INPUT_TEXT {
            let Some(ch) = char::from_u32(input.value) else { return Handed::Lost };
            (surface_text::SURFACE_TEXT_COMMIT, 0u8, &*ch.encode_utf8(&mut utf8))
        } else {
            let Ok(action) = u8::try_from(input.value) else { return Handed::Lost };
            (surface_text::SURFACE_TEXT_ACTION, action, "")
        };
        let Some((frame, len)) = surface_text::encode_surface_text(kind, aux, text, stamp) else {
            return Handed::Lost;
        };
        match key_gate::key_target(self.window_focus_target()) {
            Ok(target) => self.send_to_target(target, &frame[..len]),
            Err(_) => Handed::NoReceiver,
        }
    }

    /// A composition source's key to imed, behind the session of the field windowd has focused
    /// (or of the focused window, which hears only Escape) — one ordered outbox for both.
    fn compose_key(&mut self, input: &KeyInput) -> Handed {
        let field = self.text_focus.map(|route| (route.surface_id, route.field_kind));
        let Some(want) = key_gate::compose_session(field, self.window_focus_surface()) else {
            return Handed::NoReceiver;
        };
        if let SessionStep::Switch { close, open } =
            key_gate::session_step(self.key_route.imed_session, want)
        {
            if let Some((old, kind)) = close {
                let shut = ime_wire::encode_set_focus(u64::from(old), 0, kind, 0, 0, 0, 0);
                let _ = self.send_to_imed(&shut);
            }
            let start = ime_wire::encode_set_focus(u64::from(open.0), 1, open.1, 0, 0, 0, 0);
            if !self.send_to_imed(&start) {
                return Handed::Lost;
            }
            self.key_route.imed_session = Some(open);
        }
        let (ch, action) = if input.sub == ime_wire::KEY_KIND_ACTION {
            (0, u8::try_from(input.value).unwrap_or(0))
        } else {
            (input.value, 0)
        };
        let frame =
            ime_wire::encode_key(ime_wire::KEY_SOURCE_HW, input.sub, ch, action, input.modifiers);
        if self.send_to_imed(&frame) {
            Handed::Delivered
        } else {
            Handed::Lost
        }
    }

    /// One frame for imed on the declared leg, through the ordered outbox (ADR-0075: never a
    /// park, never a drop while imed lives).
    pub(super) fn send_to_imed(&mut self, frame: &[u8]) -> bool {
        #[cfg(nexus_env = "os")]
        {
            let leg = nexus_service_topology::slots::windowd::IMED;
            self.client_send(leg.send, frame, Delivery::Intent).accepted()
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (frame, Delivery::Intent);
            false
        }
    }

    /// The surface holding window focus (the topmost before any raise), as a delivery target.
    fn window_focus_target(&self) -> Option<text_input::TextFocusTarget> {
        match self.windows.focused().or_else(|| self.windows.top()) {
            Some(crate::window_scene::WindowId::App(i)) => {
                let idx = usize::from(i);
                self.apps[idx].surface_id.map(|_| text_input::TextFocusTarget::App(idx))
            }
            Some(crate::window_scene::WindowId::Desktop) => {
                self.desktop_surface_id.map(|_| text_input::TextFocusTarget::Desktop)
            }
            None => None,
        }
    }

    /// The window-focused surface's id.
    pub(super) fn window_focus_surface(&self) -> Option<u32> {
        match self.window_focus_target()? {
            text_input::TextFocusTarget::App(idx) => self.apps[idx].surface_id,
            text_input::TextFocusTarget::Desktop => self.desktop_surface_id,
        }
    }

    /// A text frame to `target`'s event channel, ordered (user intent: never merged, never
    /// dropped while the client lives).
    fn send_to_target(&mut self, target: text_input::TextFocusTarget, frame: &[u8]) -> Handed {
        #[cfg(nexus_env = "os")]
        {
            use super::client_send::ClientSend;
            let (channel, app) = match target {
                text_input::TextFocusTarget::App(idx) => (self.apps[idx].event_channel, Some(idx)),
                text_input::TextFocusTarget::Desktop => (self.desktop_channel, None),
            };
            let Some(channel) = channel else { return Handed::NoReceiver };
            match self.client_send(channel, frame, Delivery::Intent) {
                ClientSend::Sent | ClientSend::Queued => Handed::Delivered,
                ClientSend::Dead => {
                    // TASK-0333 6a: the peer is gone — its window closes by name.
                    if let Some(idx) = app {
                        self.note_app_channel_dead(idx);
                    }
                    Handed::NoReceiver
                }
                ClientSend::Refused => Handed::Lost,
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (target, frame);
            Handed::NoReceiver
        }
    }

    /// A key nobody could take (no window yet, a window without its channel): counted, said once.
    fn note_key_unreceived(&mut self) {
        let route = &mut self.key_route;
        route.unreceived = route.unreceived.saturating_add(1);
        if route.unreceived == 1 {
            let _ = debug_println("windowd: key input without a receiver (first)");
        }
    }

    /// A key a living receiver could not be handed: counted, the first lines named (RFC-0113 §6).
    fn note_key_lost(&mut self, reason: &str) {
        let route = &mut self.key_route;
        route.lost = route.lost.saturating_add(1);
        if route.fail_lines < FAIL_LINES_MAX {
            route.fail_lines += 1;
            let _ = debug_println(&alloc::format!(
                "WINDOWD: FAIL key input (reason={reason} lost={})",
                route.lost
            ));
        }
    }
}
