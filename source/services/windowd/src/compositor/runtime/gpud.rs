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

impl DisplayServerRuntime {
    pub(super) fn ensure_gpud_client(&mut self) -> bool {
        if self.gpud_client.is_some() {
            return true;
        }
        if let Ok(client) = KernelClient::new_for("gpud") {
            let _ = debug_println("windowd: gpud route connected");
            self.gpud_client = Some(client);
            return true;
        }
        if let Ok(client) =
            KernelClient::new_with_slots(GPUD_FALLBACK_SEND_SLOT, GPUD_FALLBACK_RECV_SLOT)
        {
            let _ = debug_println("windowd: gpud route fallback slots");
            self.gpud_client = Some(client);
            return true;
        }
        false
    }

    /// Fire-and-forget present to gpud. Pixel data is already in the VMO;
    /// gpud picks up the damage rect on its next recv iteration.
    /// Non-blocking: windowd continues processing input immediately.
    pub(super) fn send_gpud_present(&mut self, frame: &[u8]) -> bool {
        if !self.ensure_gpud_client() {
            return false;
        }
        // Drain completed present replies first so queue pressure and in-flight accounting
        // stay bounded during sustained cursor/input traffic.
        self.drain_gpud_replies();
        // Phase 6d: in-flight bound — if 2+ frames outstanding, skip this present.
        // Damage accumulates; the next successful present covers the merged region.
        const MAX_IN_FLIGHT: u32 = 2;
        if self.frames_in_flight >= MAX_IN_FLIGHT {
            return false;
        }
        let send_result = {
            let Some(client) = self.gpud_client.as_ref() else {
                return false;
            };
            client.send(frame, Wait::NonBlocking)
        };
        match send_result {
            Ok(()) => {
                self.present_seq = self.present_seq.wrapping_add(1);
                self.frames_in_flight = self.frames_in_flight.saturating_add(1);
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

    /// Drop the gpud client and reset in-flight accounting together. A stale
    /// `frames_in_flight` after a client reset would leave the counter pinned at
    /// MAX_IN_FLIGHT, blocking every future present and spinning the flush retry
    /// loop forever. Always reset both as a unit.
    pub(super) fn reset_gpud_client(&mut self) {
        self.gpud_client = None;
        self.frames_in_flight = 0;
    }

    /// Drain non-blocking gpud status replies for OP_PRESENT_DAMAGE so gpud cannot
    /// block on a full reply queue and freeze visible updates.
    pub(crate) fn drain_gpud_replies(&mut self) {
        if self.framebuffer_pending_first_write || self.gpud_client.is_none() {
            return;
        }
        // Stack-buffer drain: recv_into avoids the per-call Vec<u8> that
        // Client::recv allocates — windowd's bump allocator never frees, so a
        // per-frame reply Vec would slowly exhaust the heap.
        let mut reply_buf = [0u8; 32];
        loop {
            let recv_result = {
                let Some(client) = self.gpud_client.as_ref() else {
                    return;
                };
                client.recv_into(Wait::NonBlocking, &mut reply_buf)
            };
            match recv_result {
                Ok(n) => {
                    let status = reply_buf.get(..n).and_then(|r| r.first()).copied();
                    if status == Some(GPUD_STATUS_OK) {
                        // Present/attach replies carry a 5-byte [status, handoff_id]
                        // payload; fire-and-forget acks (cursor move) are a single
                        // status byte and must NOT be counted as present completions
                        // or they corrupt the frames-in-flight accounting.
                        if n >= 5 {
                            self.note_present_completed();
                        }
                    } else if n == 1 {
                        // Failed fire-and-forget op (cursor move). Soft-fail: drop to
                        // the software cursor path but keep the present pipeline alive.
                        if self.hw_cursor_active {
                            self.hw_cursor_active = false;
                            let _ = debug_println("windowd: hw cursor move rejected, sw fallback");
                        }
                    } else {
                        if let Some(status) = status {
                            let _ = debug_println(&alloc::format!(
                                "windowd: gpud present bad-status=0x{status:02x}"
                            ));
                        } else {
                            let _ = debug_println("windowd: gpud present bad-status=empty");
                        }
                        self.reset_gpud_client();
                        return;
                    }
                }
                Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                    return;
                }
                Err(err) => {
                    log_gpud_ipc_error("windowd: gpud present recv failed", err);
                    self.reset_gpud_client();
                    return;
                }
            }
        }
    }

    /// Blocking status request (used only for handoff/bootstrap where
    /// we must confirm gpud accepted the framebuffer VMO).
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
    pub(super) fn present_damage_to_gpud(&mut self, rect: DamageRect) -> bool {
        let frame = encode_gpud_damage_frame(rect);
        if self.send_gpud_present(&frame) {
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
        let written = match committed.serialize_into(&mut frame_buf[1..]) {
            Ok(n) => n,
            Err(_) => return self.present_damage_to_gpud(bounding),
        };
        frame_buf[0] = GPU_PRESENT_DAMAGE_OP;
        self.send_gpud_present(&frame_buf[..1 + written])
    }
}
