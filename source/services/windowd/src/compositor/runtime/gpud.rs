// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — gpud IPC client (connect/present/drain).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//!
//! Split out of `runtime/mod.rs` (TASK-0063 modularization): the
//! `DisplayServerRuntime` methods that own the gpud route — connect/fallback,
//! fire-and-forget present + reply drain (bump-heap-safe `recv_into`), the
//! blocking handoff status request, and the GPU-blur present. A child module of
//! `runtime`, so it reads the runtime's private fields directly; methods are
//! `pub(super)` so the parent and sibling submodules can still call them.

use super::*;
use nexus_display_proto::PRESENT_HEADER_LEN;

impl DisplayServerRuntime {
    /// Binds the gpud route.
    ///
    /// Routing v2 (RFC-0093 §1) makes the reply nonce mandatory, so an answer that is not
    /// ours is detected in the library and never returned here. The guards this function
    /// used to carry — rejecting an answer equal to windowd's OWN server inbox, and the
    /// matching refusal in the reply drain — were the workaround for the nonce-less
    /// protocol (an aliased answer made every gpud round-trip read client requests, so the
    /// cursor ack never matched and the boot stayed on the splash, ~1 in 2 interactive
    /// boots). They are gone with the protocol that needed them.
    ///
    /// The fallback is the same declared pair init pins before windowd resumes; the route ask in
    /// front of it only waits for gpud's readiness, which the stage fence (TASK-0324 P5) takes over.
    pub(super) fn ensure_gpud_client(&mut self) -> bool {
        if self.gpud_client.is_some() {
            return true;
        }
        // The declared gpud pair only (TASK-0324 P7-b): no route ask, gpud's readiness is the
        // stage fence's fact.
        if let Ok(client) = KernelClient::new_with_slots(GPUD_WIRED_SEND_SLOT, GPUD_WIRED_RECV_SLOT)
        {
            let _ = debug_println("windowd: gpud route wired slots");
            self.gpud_client = Some(client);
            return true;
        }
        false
    }

    /// Fire-and-forget present to gpud. Pixel data is already in the VMO;
    /// gpud picks up the damage rect on its next recv iteration.
    /// Non-blocking: windowd continues processing input immediately.
    ///
    /// `frame` holds the serialized payload at `PRESENT_HEADER_LEN..`; this writes the
    /// header — opcode + the `seq` the ack will echo (RFC-0093 §5).
    pub(super) fn send_gpud_present(&mut self, frame: &mut [u8]) -> bool {
        if !self.ensure_gpud_client() {
            return false;
        }
        // Drain completed present replies first so queue pressure and in-flight accounting
        // stay bounded during sustained cursor/input traffic.
        self.drain_gpud_replies();
        // Phase 6d: in-flight bound — if 2+ frames outstanding, skip this present.
        // Damage accumulates; the next successful present covers the merged region.
        const MAX_IN_FLIGHT: u32 = 2;
        if self.presents.in_flight() >= MAX_IN_FLIGHT {
            return false;
        }
        nexus_display_proto::write_present_header(frame, self.presents.next_seq());
        let send_result = {
            let Some(client) = self.gpud_client.as_ref() else {
                return false;
            };
            client.send(frame, Wait::NonBlocking)
        };
        match send_result {
            Ok(()) => {
                let _ = self.presents.issue();
                true
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::NoSpace) => {
                // gpud queue is currently full; caller keeps damage pending for retry.
                false
            }
            Err(err) => {
                let send_slot = self.gpud_client.as_ref().map(|c| c.slots().0).unwrap_or(0);
                log_gpud_cap_error("windowd: gpud present send failed", err, send_slot);
                self.reset_gpud_client();
                false
            }
        }
    }

    /// Drop the gpud client and the present window together: nothing outstanding can
    /// be acked on a client that is gone, and a window left full would refuse every
    /// future present.
    pub(super) fn reset_gpud_client(&mut self) {
        self.gpud_client = None;
        self.presents.reset();
        self.capture_gpud_lost();
    }

    /// The ONE interpreter of a gpud reply (RFC-0093 §5) — used by the drain and by the
    /// blocking cursor-upload path, so a present ack is credited the same way wherever it
    /// lands: only against an OUTSTANDING seq. An unknown or duplicate seq is named and
    /// never credited. Returns `false` when the client was reset (stop draining).
    pub(super) fn handle_gpud_reply(&mut self, reply: &[u8]) -> bool {
        use nexus_display_proto as proto;
        if proto::client_surface::is_client_envelope(reply) {
            // A CLIENT frame on the reply channel means this endpoint carries someone
            // else's requests after all. Stop at once — one lost frame, loudly.
            let op = reply.get(3).copied().unwrap_or(0);
            let _ = debug_println(&alloc::format!(
                "windowd: FAIL gpud reply ate client frame op={op} len={}",
                reply.len()
            ));
            self.reset_gpud_client();
            return false;
        }
        match reply.len() {
            // Fire-and-forget control acks (cursor move/select, wallpaper-dirty, reveal).
            1 => {
                if reply[0] != GPUD_STATUS_OK && self.hw_cursor_active {
                    self.hw_cursor_active = false;
                    let _ = debug_println("windowd: hw cursor move rejected, sw fallback");
                }
                true
            }
            // RFC-0095: a readback's answer — two bytes, never a cursor status or a present ack.
            proto::readback::READBACK_REPLY_LEN => {
                match proto::readback::decode_readback_reply(reply) {
                    Some(status) => self.on_capture_readback(status),
                    None => {
                        let _ = debug_println("windowd: gpud reply foreign frame len=2");
                    }
                }
                true
            }
            n if n >= proto::PRESENT_ACK_LEN => {
                let Some((status, seq)) = proto::decode_present_ack(reply) else {
                    return true;
                };
                if proto::is_cursor_reply_magic(seq) {
                    // A cursor-upload reply that outlived its blocking consumer — not a present.
                    return true;
                }
                match self.presents.ack(seq) {
                    acks::Verdict::Credited => match status {
                        GPUD_STATUS_OK => self.note_present_acked_clean(),
                        proto::STATUS_REVEALED => {
                            self.note_present_acked_clean();
                            self.on_revealed(seq);
                        }
                        _ => {
                            // gpud measured a failed/deadline-missed present: the ROUTE is
                            // healthy, the FRAME failed — requeue (bounded), never reset.
                            let _ = debug_println(&alloc::format!(
                                "windowd: gpud present nack status=0x{status:02x}"
                            ));
                            self.note_present_nacked();
                        }
                    },
                    verdict => {
                        let _ = debug_println(&alloc::format!(
                            "windowd: FAIL present ack seq={seq} unexpected ({verdict:?})"
                        ));
                    }
                }
                true
            }
            n => {
                let _ = debug_println(&alloc::format!("windowd: gpud reply foreign frame len={n}"));
                true
            }
        }
    }

    /// Drains gpud's replies (present acks, cursor/attach acks) into the in-flight
    /// accounting. The endpoint is ours by construction since routing v2 (RFC-0093 §1):
    /// the reply nonce makes an answer that is not ours impossible to accept, so the
    /// "is this actually our inbox?" guard this drain used to carry is gone with the
    /// nonce-less protocol that needed it.
    /// Drains gpud's replies. Returns whether any arrived: every one is a display-ring
    /// COMPLETION (a present ack, a layer-scroll status, a cursor status) — the frame clock
    /// of the compositor loop (TASK-0324 P7-c), which then runs [`Self::on_frame_completed`].
    pub(crate) fn drain_gpud_replies(&mut self) -> bool {
        if self.framebuffer_pending_first_write {
            return false;
        }
        if self.gpud_client.is_none() && !self.ensure_gpud_client() {
            return false;
        }
        let mut any = false;
        // Stack-buffer drain: recv_into avoids the per-call Vec<u8> that
        // Client::recv allocates — windowd's bump allocator never frees, so a
        // per-frame reply Vec would slowly exhaust the heap.
        let mut reply_buf = [0u8; 32];
        loop {
            let recv_result = {
                let Some(client) = self.gpud_client.as_ref() else {
                    return any;
                };
                client.recv_into(Wait::NonBlocking, &mut reply_buf)
            };
            match recv_result {
                Ok(n) => {
                    any = true;
                    if !self.handle_gpud_reply(&reply_buf[..n]) {
                        return any;
                    }
                }
                Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                    return any;
                }
                Err(err) => {
                    log_gpud_ipc_error("windowd: gpud present recv failed", err);
                    self.reset_gpud_client();
                    return any;
                }
            }
        }
    }

    /// Keeps the frame clock running while something animates but nothing is queued
    /// (TASK-0324 P7-c): a frame-pulse client waits for a pulse before it draws, a scroll
    /// coast advances per frame, a spring may converge without damage — with no present in
    /// flight there would be no completion and the animation would freeze. One pointer-rect
    /// present (the smallest real frame) restores the clock; its completion is the next tick.
    pub(crate) fn keep_frame_clock_alive(&mut self) {
        if self.has_pending_damage() || self.frames_in_flight() > 0 {
            return;
        }
        if self.has_active_animations()
            || self.has_frame_pulse_clients()
            || self.has_scroll_momentum()
            || self.cursor_ring_active
        {
            self.queue_cursor_damage(
                self.state.cursor_x,
                self.state.cursor_y,
                self.state.cursor_x,
                self.state.cursor_y,
            );
        }
    }

    /// One display-ring completion (TASK-0324 P7-c): the frame clock. Animations integrate
    /// real elapsed time to `now_ns`, scroll coasts advance, the wait ring steps, and the
    /// animating clients get their frame pulse — each producing the damage that the loop
    /// presents next, whose completion clocks the frame after. Idle produces nothing and the
    /// loop sleeps on its waitset.
    pub(crate) fn on_frame_completed(&mut self, now_ns: u64) {
        if self.has_active_animations() {
            self.tick(now_ns);
        }
        self.advance_app_scrolls(now_ns);
        let _ = self.cursor_wait_tick(now_ns);
        self.flush_frame_pulses();
    }

    /// Blocking control request whose reply is a bare status (layer scroll etc.).
    pub(super) fn send_gpud_status_request(&mut self, frame: &[u8]) -> Result<(), WindowdError> {
        // Drain any stale responses from previous non-blocking presents before
        // sending. Without this, client.recv(Blocking) below may pick up a
        // response meant for a different request, causing a chain of misrouted
        // status codes that corrupt the present pipeline.
        self.drain_gpud_replies();

        if !self.ensure_gpud_client() {
            return Err(WindowdError::InvalidDamage);
        }
        let send_result = {
            let client = self.gpud_client.as_ref().ok_or(WindowdError::InvalidDamage)?;
            client.send(frame, Wait::Blocking)
        };
        if let Err(err) = send_result {
            let send_slot = self.gpud_client.as_ref().map(|c| c.slots().0).unwrap_or(0);
            log_gpud_cap_error("windowd: gpud request send failed", err, send_slot);
            self.gpud_client = None;
            return Err(WindowdError::InvalidDamage);
        }
        let recv_result = {
            let client = self.gpud_client.as_ref().ok_or(WindowdError::InvalidDamage)?;
            client.recv(Wait::Blocking)
        };
        match recv_result {
            Ok(reply) if reply.first().copied() == Some(GPUD_STATUS_OK) => Ok(()),
            Ok(reply) => {
                if let Some(status) = reply.first().copied() {
                    let _ = debug_println(&alloc::format!(
                        "windowd: gpud request bad-status=0x{status:02x}"
                    ));
                } else {
                    let _ = debug_println("windowd: gpud request bad-status=empty");
                }
                self.gpud_client = None;
                Err(WindowdError::InvalidDamage)
            }
            Err(err) => {
                log_gpud_ipc_error("windowd: gpud request recv failed", err);
                self.gpud_client = None;
                Err(WindowdError::InvalidDamage)
            }
        }
    }

    /// Fire-and-forget: sends a frame to gpud without waiting or tracking.
    /// Used for non-critical operations (cursor upload) where the response
    /// is drained by drain_gpud_replies() on the next loop iteration.
    /// Does NOT increment frames_in_flight — not a present.
    pub(super) fn send_gpud_fire_forget(&mut self, frame: &[u8]) -> bool {
        self.drain_gpud_replies();
        if !self.ensure_gpud_client() {
            return false;
        }
        let Some(client) = self.gpud_client.as_ref() else {
            return false;
        };
        client.send(frame, Wait::NonBlocking).is_ok()
    }

    /// Non-blocking: sends damage rect to gpud and returns immediately.
    /// Pixel data is already written to the VMO by CPU compositing.
    /// gpud processes the damage asynchronously — windowd continues its loop.
    /// (Damage-rect present path — superseded by the whole-scene CB batch;
    /// kept for the compositor-scroll/damage track, see
    /// plans/webrender-compositor-scroll.md.)
    #[allow(dead_code)]
    pub(super) fn present_damage_to_gpud(&mut self, rect: DamageRect) -> bool {
        let mut frame = encode_gpud_damage_frame(rect);
        if self.send_gpud_present(&mut frame) {
            self.present_fail_reported = false;
            return true;
        }
        // Rate-limited: once per failure episode, not every retry (the retry path
        // runs at ~120 Hz during backpressure and would flood the UART log — the
        // very stall the watchdog reports cleanly).
        if !self.present_fail_reported {
            let _ = debug_println("windowd: gpud present damage failed (non-blocking, will retry)");
            self.present_fail_reported = true;
        }
        false
    }

    /// Build and send a GPU-first frame that includes BlurBackdrop commands
    /// for the glass panel region. gpud executes the blur over the CPU-composited
    /// base scene, replacing the CPU blur path in `backdrop.rs`.
    ///
    /// Phase 2: GPU-first glass panel (Workstreams 1+4).
    /// The BlurBackdrop command samples from the VMO at `DISPLAY_OFFSET_BYTES`,
    /// applies a box blur + saturation, and writes back.
    /// (GPU-first blur present variant — superseded by the virgl in-scene blur;
    /// kept: documents the BlurBackdrop wire usage.)
    #[allow(dead_code)]
    pub(super) fn present_frame_with_gpu_blur(&mut self, bounding: DamageRect) -> bool {
        let mut cmd = CommandBuffer::new();
        {
            let mut encoder = match cmd.try_begin_render_pass(RenderPassDesc {
                color_attachments: alloc::vec![],
                width: self.mode.width,
                height: self.mode.height,
            }) {
                Ok(e) => e,
                Err(_) => return false,
            };
            // Blur the combined glass panel region.
            // gpud reads from the VMO display region (offset DISPLAY_OFFSET_BYTES),
            // applies box blur, and writes the result back.
            let glass_rect =
                TileRect { x: 0, y: 0, width: COMBINED_PANEL_WIDTH as u32, height: PROOF_PANEL_H };
            if encoder
                .try_blur_backdrop(
                    glass_rect,
                    DARK_GLASS_BLUR_RADIUS,
                    DARK_GLASS_SATURATION_PERCENT,
                )
                .is_err()
            {
                // Fall back to simple damage rect if command buffer fails.
                return self.present_damage_to_gpud(bounding);
            }
            encoder.end_encoding();
        }
        let committed = match cmd.try_commit() {
            Ok(c) => c,
            Err(_) => return self.present_damage_to_gpud(bounding),
        };
        let mut frame_buf = [0u8; 256];
        let written = match committed.serialize_into(&mut frame_buf[PRESENT_HEADER_LEN..]) {
            Ok(n) => n,
            Err(_) => return self.present_damage_to_gpud(bounding),
        };
        self.send_gpud_present(&mut frame_buf[..PRESENT_HEADER_LEN + written])
    }
}
