// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd's screen-capture verb (RFC-0095, ADR-0071, TASK-0068) — the runtime half of
//! `crate::capture_gate`, which decides; this file talks to screencapd and gpud and applies the
//! freeze. screencapd's request (`OP_SURFACE_CAPTURE`) parks its reply capability here until
//! gpud answers the readback; the frame VMO screencapd lent (`ATTACH`) stays here and a clone
//! rides each readback. The pointer leaves the frame through the paths that draw it (the GL
//! build-up via a negative `OP_MOVE_CURSOR`, the software blend by skipping `BlendCursor`) and
//! returns with the readback. Every send that must not be lost — the hide, the readback, the
//! pointer's return, the thaw's wallpaper re-upload — is retried by the loop's pass until gpud's
//! queue takes it: a dropped thaw would leave the frozen frame as the wallpaper.
//! windowd draws nothing here; the capture UI is the shell's.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the decisions in `tests/capture_gate.rs`; the wire path via the QEMU markers
//!   `windowd: capture freeze on` / `off` and `SELFTEST: ui v7 screencap ok`.
//! RFC: docs/rfcs/RFC-0095-screen-capture-v1-readback-freeze-shell-ui.md

use super::*;
use crate::capture_gate::{self as gate, Plan, Step};
use nexus_display_proto::readback::{self as rb, Readback, READBACK_FREEZE};
use nexus_display_proto::surface_capture as wire;

/// The sends a capture owes gpud until its queue takes them.
#[derive(Clone, Copy, Default)]
struct Owed {
    /// The pointer's return (GL: its real position).
    show: bool,
    /// The readback itself.
    readback: Option<Readback>,
    /// `OP_WALLPAPER_DIRTY` after a thaw (GL: the wallpaper texture holds the frozen frame).
    thaw: bool,
}

/// The capture state the runtime carries.
pub(super) struct CaptureState {
    machine: gate::Machine,
    /// screencapd's frame VMO (`ATTACH`): kept, never mapped; a clone rides each readback.
    vmo: Option<u32>,
    /// The request waiting for gpud: its reply capability and nonce.
    parked: Option<(nexus_ipc::ReplyCap, u32)>,
    /// The reply body, snapshotted when the readback was sent (the frozen moment).
    reply: wire::CaptureReply,
    windows: [wire::CaptureWindow; wire::CAPTURE_WINDOWS_MAX],
    window_count: usize,
    owed: Owed,
    /// The read in progress takes the pointer out of the frame (the display draws it in).
    hides: bool,
    /// Refusal reasons already named (one line each per boot — a foreign sender cannot flood).
    denials_said: u8,
    /// A capture key the desktop surface has not taken yet, and the push sequence.
    key_owed: Option<u8>,
    key_seq: u32,
}

impl CaptureState {
    pub(super) fn new() -> Self {
        Self {
            machine: gate::Machine::new(),
            vmo: None,
            parked: None,
            reply: wire::CaptureReply::default(),
            windows: [wire::CaptureWindow::default(); wire::CAPTURE_WINDOWS_MAX],
            window_count: 0,
            owed: Owed::default(),
            hides: false,
            denials_said: 0,
            key_owed: None,
            key_seq: 0,
        }
    }

    /// Windows and overlays are out of the composition (the frozen frame shows them) — from
    /// the moment the freeze readback is in gpud's queue, never before.
    pub(super) fn frozen(&self) -> bool {
        self.machine.frozen() && self.owed.readback.is_none()
    }

    /// The pointer is withheld from the frame — until the readback is in gpud's queue.
    pub(super) fn pointer_hidden(&self) -> bool {
        self.machine.pointer_hidden() || (self.hides && self.owed.readback.is_some())
    }
}

impl DisplayServerRuntime {
    /// `OP_SURFACE_CAPTURE` (screencapd → windowd): admit, then act or park the reply.
    pub(crate) fn handle_surface_capture(
        &mut self,
        frame: &[u8],
        moved: Option<nexus_ipc::ReplyCap>,
        sender_sid: u64,
    ) {
        let Some(req) = wire::decode_capture_request(frame) else {
            self.capture_refuse(moved, wire::CAPTURE_MALFORMED, 0, false);
            return;
        };
        let facts = gate::Facts {
            from_screencapd: sender_sid == nexus_abi::service_id_from_name(b"screencapd"),
            greeter: self.greeter_active() || !self.session_resolved(),
            mode: self.framebuffer.map(|_| (self.mode.width, self.mode.height)),
            vmo: self.capture.vmo.is_some(),
            pointer_in_frame: !self.hw_cursor_active,
        };
        let attach = req.cmd == wire::CAPTURE_ATTACH;
        let plan = match gate::admit(&req, &facts, self.capture.machine.phase()) {
            Ok(plan) => plan,
            Err(status) => {
                self.capture_refuse(moved, status, req.nonce, attach);
                return;
            }
        };
        match plan {
            Plan::Attach => {
                // The moved capability IS the VMO (the gpud-attach pattern): keep its slot.
                let Some(cap) = moved else { return };
                let slot = cap.slot();
                core::mem::forget(cap);
                if let Some(old) = self.capture.vmo.replace(slot) {
                    let _ = nexus_abi::cap_close(old);
                }
            }
            Plan::Thaw => {
                self.capture.machine.thaw();
                self.capture_thaw_effects();
                if let Some(cap) = moved {
                    self.capture_reply_on(cap, wire::CAPTURE_OK, req.nonce, false);
                }
            }
            Plan::Read { readback, hide_pointer } => {
                let Some(cap) = moved else {
                    // A read nobody can be answered for is refused before it touches anything.
                    self.capture_say_denial(wire::CAPTURE_MALFORMED);
                    return;
                };
                self.capture.parked = Some((cap, req.nonce));
                self.capture.hides = hide_pointer;
                let now = nsec().unwrap_or(0);
                let hide_seq = if hide_pointer { self.capture_hide_pointer() } else { None };
                let step = self.capture.machine.start(readback, hide_pointer, hide_seq, now);
                self.capture_step(step);
            }
        }
    }

    /// gpud answered the readback (`[status, OP_READBACK]`).
    pub(super) fn on_capture_readback(&mut self, status: u8) {
        let step = self.capture.machine.readback_done(status, nsec().unwrap_or(0));
        if let Step::Answer { status: wire::CAPTURE_OK, .. } = step {
            if self.capture.machine.frozen() {
                let _ = debug_println("windowd: capture freeze on");
            }
        }
        self.capture_step(step);
    }

    /// The gpud route went away: a read in flight cannot be answered by gpud any more.
    pub(super) fn capture_gpud_lost(&mut self) {
        if matches!(self.capture.machine.phase(), gate::Phase::Reading { .. }) {
            let step = self.capture.machine.send_failed();
            self.capture_step(step);
        }
    }

    /// The loop's capture pass (cheap when idle): owed sends, the hidden-pointer present, the
    /// bounds.
    pub(crate) fn pump_capture(&mut self, now: u64) {
        if self.capture.key_owed.is_some() {
            self.send_capture_key();
        }
        let owed = self.capture.owed;
        if self.capture.machine.phase() == gate::Phase::Idle && !owed.thaw && !owed.show {
            return;
        }
        if owed.thaw {
            self.capture.owed.thaw = !self.capture_send_wallpaper_dirty();
        }
        if self.capture.machine.hide_owed() {
            if let Some(seq) = self.capture_hide_pointer() {
                self.capture.machine.hide_sent(seq);
            }
        }
        if owed.show {
            self.capture_show_pointer();
        }
        let step = self.capture.machine.presented(self.presents.last_issued(), now);
        self.capture_step(step);
        if let Some(readback) = self.capture.owed.readback {
            self.capture_send_readback(readback);
        }
        let step = self.capture.machine.expire(now);
        self.capture_step(step);
    }

    /// A capture key from inputd (Print, Shift+Print, Alt+Print): handed to the desktop
    /// surface — the shell owns the screenshot UI — as `OP_SURFACE_CAPTURE_KEY`, retried by the
    /// loop's pass until its channel takes it. No line per key: a key press is the user's.
    pub(super) fn push_capture_key(&mut self, kind: u8) {
        self.capture.key_seq = self.capture.key_seq.wrapping_add(1);
        self.capture.key_owed = Some(kind);
        self.send_capture_key();
    }

    fn send_capture_key(&mut self) {
        let (Some(kind), Some(slot)) = (self.capture.key_owed, self.desktop_channel) else {
            return; // owed until the desktop surface is bound
        };
        let frame = wire::encode_capture_key(kind, self.capture.key_seq);
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        if nexus_abi::ipc_send_v1(slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0).is_ok() {
            self.capture.key_owed = None;
        }
    }

    fn capture_step(&mut self, step: Step) {
        match step {
            Step::Wait => {}
            Step::SendReadback(readback) => {
                self.capture_snapshot(&readback);
                if readback.flags & READBACK_FREEZE != 0 {
                    // Every present behind the readback lands over the frozen base: compose
                    // the screen once more without the windows.
                    self.queue_full_frame_damage();
                }
                self.capture_send_readback(readback);
            }
            Step::Answer { status, thaw } => {
                if thaw {
                    self.capture_thaw_effects();
                }
                if let Some((cap, nonce)) = self.capture.parked.take() {
                    self.capture_reply_on(cap, status, nonce, status == wire::CAPTURE_OK);
                }
            }
            Step::Expired => {
                let _ = debug_println("windowd: capture freeze expired");
                self.capture_thaw_effects();
            }
        }
    }

    /// The reply body at the frozen moment; for a freeze also the pointer's sprite, written
    /// behind the frame in the VMO (the compositor's sprite — screencapd keeps no cursor copy).
    fn capture_snapshot(&mut self, readback: &Readback) {
        let mut reply = wire::CaptureReply { w: readback.w, h: readback.h, ..Default::default() };
        self.capture.window_count = 0;
        if readback.flags & READBACK_FREEZE != 0 {
            let (sprite, w, h, hot_x, hot_y) = self.cursor_shape.sprite();
            let side = wire::CAPTURE_SPRITE_MAX as u32;
            let bytes = w as usize * h as usize * 4;
            let at = readback.bytes();
            let written = w <= side
                && h <= side
                && sprite.len() >= bytes
                && self.capture.vmo.is_some_and(|vmo| vmo_write(vmo, at, &sprite[..bytes]).is_ok());
            let clamp = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
            reply.pointer = wire::CapturePointer {
                x: clamp(self.state.cursor_x),
                y: clamp(self.state.cursor_y),
                hot_x: hot_x.clamp(0, 255) as u8,
                hot_y: hot_y.clamp(0, 255) as u8,
                w: if written { w as u8 } else { 0 },
                h: if written { h as u8 } else { 0 },
            };
            let osk = self.osk_idx();
            let (order, n) = self.windows.hit_order(false);
            let live = order[..n].iter().filter_map(|id| match *id {
                crate::window_scene::WindowId::App(i) if Some(usize::from(i)) != osk => {
                    let slot = &self.apps[usize::from(i)];
                    let win = &slot.win;
                    slot.surface_id.map(|sid| (sid, win.x, win.y, win.w, win.h))
                }
                _ => None,
            });
            let (windows, count) = gate::capture_windows(live);
            self.capture.windows = windows;
            self.capture.window_count = count;
        }
        self.capture.reply = reply;
    }

    /// Sends the readback with a clone of the frame VMO; a full queue owes it to the next pass.
    fn capture_send_readback(&mut self, readback: Readback) {
        self.capture.owed.readback = None;
        let (Some(vmo), true) = (self.capture.vmo, self.ensure_gpud_client()) else {
            let step = self.capture.machine.send_failed();
            self.capture_step(step);
            return;
        };
        let Some(send_slot) = self.gpud_client.as_ref().map(|c| c.slots().0) else { return };
        let Ok(clone) = nexus_abi::cap_clone(vmo) else {
            let step = self.capture.machine.send_failed();
            self.capture_step(step);
            return;
        };
        let frame = rb::encode_readback(&readback);
        match nexus_ipc::exchange::send_with_cap_nonblocking(send_slot, &frame, clone) {
            // The frames behind the readback in gpud's queue show the pointer again.
            Ok(()) => {
                if core::mem::take(&mut self.capture.hides) {
                    self.capture_show_pointer();
                }
            }
            Err(nexus_ipc::IpcError::NoSpace)
            | Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::QueueFull)) => {
                let _ = nexus_abi::cap_close(clone);
                self.capture.owed.readback = Some(readback);
            }
            Err(_) => {
                let _ = nexus_abi::cap_close(clone);
                let step = self.capture.machine.send_failed();
                self.capture_step(step);
            }
        }
    }

    /// Takes the pointer out of the frame; returns the seq the next present will carry once the
    /// hide is on its way (`None`: gpud's queue was full, the pass retries).
    fn capture_hide_pointer(&mut self) -> Option<u32> {
        if self.gl_cursor_active {
            // `send_cursor_move_to_gpud` sends the "no pointer" position while hiding.
            let mut frame = [0u8; 9];
            frame[0] = GPU_MOVE_CURSOR_OP;
            frame[1..5].copy_from_slice(&(-1i32).to_le_bytes());
            frame[5..9].copy_from_slice(&(-1i32).to_le_bytes());
            if !self.send_gpud_fire_forget(&frame) {
                return None;
            }
        }
        let (x, y) = (self.state.cursor_x, self.state.cursor_y);
        self.queue_cursor_damage(x, y, x, y);
        Some(self.presents.next_seq())
    }

    /// Puts the pointer back into the frames behind the readback.
    fn capture_show_pointer(&mut self) {
        self.capture.owed.show = false;
        if self.gl_cursor_active {
            let x = self.state.cursor_x.clamp(0, self.mode.width.saturating_sub(1) as i32);
            let y = self.state.cursor_y.clamp(0, self.mode.height.saturating_sub(1) as i32);
            let mut frame = [0u8; 9];
            frame[0] = GPU_MOVE_CURSOR_OP;
            frame[1..5].copy_from_slice(&x.to_le_bytes());
            frame[5..9].copy_from_slice(&y.to_le_bytes());
            self.capture.owed.show = !self.send_gpud_fire_forget(&frame);
        }
        let (x, y) = (self.state.cursor_x, self.state.cursor_y);
        self.queue_cursor_damage(x, y, x, y);
    }

    /// The frozen frame leaves both bases: plane 1 is re-rendered from the wallpaper source
    /// (CPU paths), the wallpaper texture re-uploaded from plane 0 (GL); windows return.
    fn capture_thaw_effects(&mut self) {
        self.capture.owed.thaw = !self.capture_send_wallpaper_dirty();
        self.queue_dirty_rect(DamageRect {
            x: 0,
            y: 0,
            width: self.mode.width,
            height: self.mode.height,
        });
        self.queue_full_frame_damage();
        let _ = debug_println("windowd: capture freeze off");
    }

    fn capture_send_wallpaper_dirty(&mut self) -> bool {
        self.send_gpud_fire_forget(&[nexus_display_proto::OP_WALLPAPER_DIRTY])
    }

    /// Refuses a request: a moved VMO (`ATTACH`) is closed, a reply capability answered.
    fn capture_refuse(
        &mut self,
        moved: Option<nexus_ipc::ReplyCap>,
        status: u8,
        nonce: u32,
        attach: bool,
    ) {
        self.capture_say_denial(status);
        match moved {
            Some(cap) if attach => cap.close(),
            Some(cap) => self.capture_reply_on(cap, status, nonce, false),
            None => {}
        }
    }

    fn capture_say_denial(&mut self, status: u8) {
        if status == wire::CAPTURE_NO_DISPLAY {
            return; // nothing to read on a display-less system: an answer, not a refusal
        }
        let bit = 1u8 << status.min(7);
        if self.capture.denials_said & bit != 0 {
            return;
        }
        self.capture.denials_said |= bit;
        let _ = debug_println(match status {
            wire::CAPTURE_DENIED => "windowd: capture deny (reason=identity-or-greeter)",
            wire::CAPTURE_BUSY => "windowd: capture deny (reason=busy)",
            wire::CAPTURE_FAILED => "windowd: capture deny (reason=no-frame)",
            _ => "windowd: capture deny (reason=malformed)",
        });
    }

    /// Answers on `cap`: the status, with the snapshotted body (size, pointer, windows) when
    /// `body` — a read that completed.
    fn capture_reply_on(&mut self, cap: nexus_ipc::ReplyCap, status: u8, nonce: u32, body: bool) {
        let (reply, n) = if body {
            (wire::CaptureReply { status, nonce, ..self.capture.reply }, self.capture.window_count)
        } else {
            (wire::CaptureReply { status, nonce, ..Default::default() }, 0)
        };
        let mut out = [0u8; wire::CAPTURE_REPLY_MAX_LEN];
        let windows = &self.capture.windows[..n.min(wire::CAPTURE_WINDOWS_MAX)];
        match wire::encode_capture_reply(&reply, windows, &mut out) {
            Some(len) => {
                let _ = cap.reply_and_close_wait(&out[..len], Wait::NonBlocking);
            }
            None => cap.close(),
        }
    }
}
