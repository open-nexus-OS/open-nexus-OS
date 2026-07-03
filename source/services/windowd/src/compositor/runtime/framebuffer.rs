// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — framebuffer/VMO registration, the first-frame handoff to gpud, and bootstrap-frame writes.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (behavior covered via windowd QEMU smoke + host integration)
//!
//! Split out of `runtime/mod.rs` (TASK-0063 modularization). A child module of
//! `runtime`, so these `impl DisplayServerRuntime` methods read the runtime's
//! private fields directly; previously-private methods are widened to
//! `pub(super)` so the parent and sibling submodules can still call them.

use super::*;

impl DisplayServerRuntime {
    /// Phase 6c: Write source frame (wallpaper) to VMO bottom half once.
    /// Moves 4MB of pixel data from control-plane heap to data-plane VMO.
    /// Banded: ROW_WRITE_CHUNK rows per `vmo_write` (same pattern as
    /// `write_fast_bootstrap_frame`) instead of one syscall per row — the
    /// wallpaper is 800 rows, and 800 kernel round-trips sat on windowd's
    /// first-frame path.
    pub(crate) fn write_source_frame_to_vmo(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        if self.source_frame.pixels.is_empty()
            || self.source_frame.width == 0
            || self.source_frame.height == 0
        {
            return Ok(());
        }
        let src_stride = self.source_frame.stride as usize;
        let dst_stride = DISPLAY_WIDTH as usize * 4;
        let copy_len = (self.source_frame.width as usize * 4).min(src_stride).min(dst_stride);
        let rows = self.source_frame.height.min(DISPLAY_HEIGHT) as usize;
        let mut band_start = 0usize;
        while band_start < rows {
            let band_end = (band_start + ROW_WRITE_CHUNK).min(rows);
            let band_rows = band_end - band_start;
            let band_bytes = band_rows * dst_stride;
            let band = &mut self.band_scratch[..band_bytes];
            band.fill(0);
            for row_idx in 0..band_rows {
                let src_off = (band_start + row_idx) * src_stride;
                band[row_idx * dst_stride..row_idx * dst_stride + copy_len]
                    .copy_from_slice(&self.source_frame.pixels[src_off..src_off + copy_len]);
            }
            vmo_write(handle, band_start * dst_stride, band)
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }

    /// Phase 1 of framebuffer registration: store the VMO handle and set
    /// display-ready flags. Returns immediately so the IPC response
    /// is not blocked by the expensive first-frame write.
    ///
    /// Phase 2 (write_current_frame + marker emissions) happens deferred
    /// via `process_deferred_framebuffer_write()`.
    pub(crate) fn register_framebuffer_vmo(&mut self, handle: Handle) {
        self.framebuffer = Some(handle);
        self.framebuffer_pending_first_write = true;
        let next = self.first_handoff_id.wrapping_add(1);
        self.first_handoff_id = if next == 0 { 1 } else { next };
        self.first_handoff_deadline_ns =
            nsec().ok().map(|now| now.saturating_add(FIRST_HANDOFF_DEADLINE_NS)).unwrap_or(0);
        self.first_handoff_frame_written = false;
        self.first_handoff_bootstrap_markers_emitted = false;
        self.first_handoff_attach_acked = false;
        self.first_handoff_present_sent = false;
    }

    /// Phase D.1: true while first-frame handoff is still in progress.
    pub(crate) fn is_handoff_pending(&self) -> bool {
        self.framebuffer_pending_first_write
    }

    /// Phase 6d: called when gpud acknowledges a present (blocking reply received).
    pub(super) fn note_present_completed(&mut self) {
        self.last_completed_seq = self.present_seq;
        self.frames_in_flight = self.frames_in_flight.saturating_sub(1);
        // Display stays SINGLE-buffered (slot A, rows 1600..2399). The old
        // per-ack slot toggle was a half-wired experiment: gpud NEVER switched
        // its scanout/upload row off slot A, so every second frame was blitted
        // into invisible memory — and "slot B" (offset 12_288_000 = row 2400)
        // actually aliases Plane 3, the blur cache. One-shot presents (the
        // login greeter reveal) landed there deterministically and never
        // showed (TASK-0065B regression hunt, 2026-07-03). Real page flipping
        // needs a gpud-side scanout switch + its own plane — not this.
    }

    /// Phase 4: byte offset into VMO for the current display slot.
    pub(super) fn current_display_offset(&self) -> usize {
        if self.current_display_slot == 0 {
            super::DISPLAY_OFFSET_BYTES
        } else {
            super::DISPLAY_SLOT_B_OFFSET_BYTES
        }
    }

    /// Phase 2 of framebuffer registration: write the first composed frame
    /// and emit all bootstrap markers. Called from the IPC loop after the
    /// VMO-ack response has been sent.
    pub(crate) fn process_deferred_framebuffer_write(&mut self) -> u8 {
        if !self.framebuffer_pending_first_write {
            return STATUS_OK;
        }
        if self.first_handoff_deadline_ns != 0 {
            let now = nsec().unwrap_or(0);
            if now >= self.first_handoff_deadline_ns {
                let _ = debug_println("windowd: ERROR first-frame handoff timeout");
                self.framebuffer_pending_first_write = false;
                return STATUS_MALFORMED;
            }
        }
        let Some(handle) = self.framebuffer else {
            let _ = debug_println("windowd: ERROR framebuffer missing during handoff");
            self.framebuffer_pending_first_write = false;
            return STATUS_MALFORMED;
        };

        if !self.first_handoff_frame_written {
            if let Err(err) = self.write_current_frame() {
                let _ = debug_println(&alloc::format!(
                    "windowd: ERROR first-frame write failed err={:?}",
                    err
                ));
                self.framebuffer_pending_first_write = false;
                return STATUS_MALFORMED;
            }
            self.first_handoff_frame_written = true;
        }

        if !self.first_handoff_bootstrap_markers_emitted {
            let _ = debug_println(LAYOUT_ENGINE_ON_MARKER);
            let _ = debug_println(TEXT_WRAPPING_ON_MARKER);
            let _ = debug_println(DISPLAY_BOOTSTRAP_MARKER);
            let _ = debug_println(DISPLAY_MODE_MARKER);
            let _ = debug_println(VISIBLE_BACKEND_MARKER);
            let _ = debug_println(COMPOSE_READY_MARKER);
            let _ = debug_println(PRESENT_QUEUED_MARKER);
            self.first_handoff_bootstrap_markers_emitted = true;
        }

        // Reactive handoff: block until gpud accepts the VMO (no polling).
        if !self.first_handoff_attach_acked {
            self.do_handoff_attach_blocking(handle);
        }

        // Session decision BEFORE the first present (TASK-0065B): sessiond is
        // ready long before this point, so the first revealed frame already
        // shows the login greeter (or the session shell) — the boot-default
        // desktop never flashes. Bounded; a miss defers to the cadenced probe.
        if self.first_handoff_attach_acked && !self.first_handoff_present_sent {
            self.session_probe_at_handoff();
        }

        // Reactive present: blit the full retained scene to the display plane and
        // overlay the cursor, then block until ack. The CPU composite above wrote
        // the scene into Plane 1; this CB copies it to Plane 2 (display) so the
        // first frame is identical to every steady-state frame (one code path).
        if !self.first_handoff_present_sent {
            let full = DamageRect { x: 0, y: 0, width: self.mode.width, height: self.mode.height };
            let mut frame_buf = [0u8; 8192];
            let sent = match self.build_scene_cb_into(&[full], 1, &mut frame_buf[1..]) {
                Ok(written) => {
                    frame_buf[0] = GPU_PRESENT_DAMAGE_OP;
                    Some(self.send_gpud_present(&frame_buf[..1 + written]))
                }
                Err(_) => None,
            };
            if sent == Some(true) {
                let _ = debug_println("windowd: handoff present sent");
                self.first_handoff_present_sent = true;
                // Drain the ack reply (kernel delivers it reactively).
                self.drain_gpud_replies();
                // Proof-harness contract (TASK-0055/0055B): first checked
                // present — one full-screen damage rect, sequence 1.
                let _ = debug_println("windowd: present ok (seq=1 dmg=1)");
            } else {
                let _ = debug_println("windowd: handoff present failed");
                self.framebuffer_pending_first_write = false;
                return STATUS_MALFORMED;
            }
        }

        self.state.display_scanout_ready = true;
        self.state.systemui_first_frame_visible = true;
        self.refresh_observer_state();
        let _ = debug_println(PRESENT_SCHEDULER_ON_MARKER);
        // Bring-up done (present scheduler on) — flush windowd's folded markers as one
        // `windowd N/N OK <ms>` grid line, then stop folding (later per-frame markers print raw).
        nexus_abi::service_verdict_flush("windowd");
        self.input_markers_emitted.scheduler = true;
        let _ = debug_println(SELFTEST_UI_V2_PRESENT_OK_MARKER);
        self.input_markers_emitted.v2_present = true;
        let _ = debug_println(DISPLAY_FIRST_SCANOUT_MARKER);
        let _ = debug_println(SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER);
        let _ = debug_println(PRESENT_VISIBLE_MARKER);
        let _ = debug_println(SELFTEST_UI_VISIBLE_PRESENT_MARKER);
        self.emit_asset_markers();
        // First frame IS a real composition — set verified so emit_v3b_markers()
        // fires. The gate checks v3b_composition_verified before emitting.
        self.v3b_composition_verified = true;
        self.emit_v3b_markers();
        // Upload cursor sprite to gpud for software BlendCursor compositing.
        // This is a software-side sprite (not a hardware cursor resource), so it
        // avoids the QEMU virtio-gpu quirk where UPDATE_CURSOR corrupts RESOURCE_FLUSH.
        if self.state.cursor_svg_visible {
            self.upload_cursor_bitmap_to_gpud();
        }
        // The standalone test icon sprite (TASK #61) is retired — the shell's
        // chrome (topbar + chat) is the real UI now. `upload_icon_to_gpud`
        // remains available for when the topbar hosts a real app icon (P3).
        self.framebuffer_pending_first_write = false;
        STATUS_OK
    }

    /// Reactive handoff: send VMO to gpud and block until acknowledged.
    /// No polling — the kernel wakes us when gpud's reply arrives.
    pub(super) fn do_handoff_attach_blocking(&mut self, fb_handle: Handle) {
        if !self.ensure_gpud_client() {
            let _ = debug_println("windowd: handoff no gpud client");
            return;
        }
        let clone = match nexus_abi::cap_clone(fb_handle) {
            Ok(cap) => cap,
            Err(_) => {
                let _ = debug_println("windowd: handoff cap-clone failed");
                return;
            }
        };
        let frame = encode_gpud_attach_frame(self.first_handoff_id);
        let send_ok = {
            let Some(client) = self.gpud_client.as_ref() else {
                let _ = nexus_abi::cap_close(clone);
                return;
            };
            match client.send_with_cap_move_wait(&frame, clone, Wait::Blocking) {
                Ok(()) => true,
                Err(e) => {
                    log_gpud_cap_error("windowd: handoff cap-move send failed", e, client.slots().0);
                    self.gpud_client = None;
                    false
                }
            }
        };
        if !send_ok {
            return;
        }
        let _ = debug_println("windowd: handoff attach sent");
        // Block until gpud responds — fully reactive, no polling.
        let ack_ok = {
            let Some(client) = self.gpud_client.as_ref() else {
                return;
            };
            match client.recv(Wait::Blocking) {
                Ok(reply) => reply.first().copied() == Some(GPUD_STATUS_OK),
                Err(e) => {
                    log_gpud_ipc_error("windowd: handoff ack recv failed", e);
                    self.gpud_client = None;
                    false
                }
            }
        };
        if ack_ok {
            let _ = debug_println("windowd: handoff attach ack");
            // Proof-harness contract (TASK-0055B): the acked VMO handoff is
            // exactly what this marker asserts.
            let _ = debug_println("windowd: fb handoff to gpud ok");
            self.first_handoff_attach_acked = true;
        } else {
            let _ = debug_println("windowd: handoff attach ack bad status");
        }
    }

    pub(super) fn write_fast_bootstrap_frame(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let row_len = self.mode.stride as usize;
        let width = self.mode.width as usize;
        let height = self.mode.height as usize;
        if row_len < width.saturating_mul(4) {
            return Err(WindowdError::BufferLengthMismatch);
        }

        let win_w = 820usize;
        let win_h = 460usize;
        let win_x = (width.saturating_sub(win_w)) / 2;
        let win_y = (height.saturating_sub(win_h)) / 2;
        let title_h = 56usize;

        let bg = [18u8, 18u8, 18u8, 255u8];
        let panel = [42u8, 46u8, 54u8, 255u8];
        let bar = [64u8, 74u8, 92u8, 255u8];
        let border = [84u8, 106u8, 144u8, 255u8];

        let mut band_start = 0usize;
        while band_start < height {
            let band_end = (band_start + ROW_WRITE_CHUNK).min(height);
            let band_rows = band_end - band_start;
            let band_bytes = band_rows * row_len;
            let band = &mut self.band_scratch[..band_bytes];
            band.fill(0);
            for row_idx in 0..band_rows {
                let y = band_start + row_idx;
                let row = &mut band[row_idx * row_len..(row_idx + 1) * row_len];
                for px in row[..width * 4].chunks_exact_mut(4) {
                    px.copy_from_slice(&bg);
                }
                if y >= win_y && y < win_y + win_h {
                    let in_border_y = y == win_y || y + 1 == win_y + win_h;
                    for x in win_x..(win_x + win_w) {
                        let idx = x * 4;
                        let in_border_x = x == win_x || x + 1 == win_x + win_w;
                        let color = if in_border_x || in_border_y {
                            border
                        } else if y < win_y + title_h {
                            bar
                        } else {
                            panel
                        };
                        row[idx..idx + 4].copy_from_slice(&color);
                    }
                }
            }
            vmo_write(handle, DISPLAY_OFFSET_BYTES + band_start * row_len, band)
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }

    /// Returns true when at least one animation is active and needs driving.
    /// Send the framebuffer VMO to gpud for zero-copy GPU scanout.
    /// Returns true only after gpud accepted the VMO handoff.
    pub(super) fn try_handoff_framebuffer_to_gpud(&mut self, fb_handle: Handle) -> bool {
        if !self.ensure_gpud_client() {
            return false;
        }

        // Single-shot clone: bootstrap is fail-fast by design.
        let clone = match nexus_abi::cap_clone(fb_handle) {
            Ok(c) => c,
            Err(_) => {
                let _ = debug_println("windowd: fb handoff to gpud cap-clone failed");
                return false;
            }
        };

        // Send VMO with blocking wait — kernel guarantees delivery before return.
        let request = [GPU_SET_FRAMEBUFFER_VMO_OP];
        let send_result = {
            let Some(client) = self.gpud_client.as_ref() else {
                return false;
            };
            client.send_with_cap_move_wait(&request, clone, Wait::Blocking)
        };
        let recv_result = if send_result.is_ok() {
            let Some(client) = self.gpud_client.as_ref() else {
                return false;
            };
            client.recv(Wait::Blocking)
        } else {
            Err(nexus_ipc::IpcError::Disconnected)
        };
        match (send_result, recv_result) {
            (Ok(()), Ok(reply)) if reply.first().copied() == Some(GPUD_STATUS_OK) => {
                let _ = debug_println("windowd: fb handoff to gpud ok");
                true
            }
            (Ok(()), Ok(reply)) => {
                if let Some(status) = reply.first().copied() {
                    let _ = debug_println(&alloc::format!(
                        "windowd: fb handoff to gpud bad-status=0x{status:02x}"
                    ));
                } else {
                    let _ = debug_println("windowd: fb handoff to gpud bad-status=empty");
                }
                self.gpud_client = None;
                false
            }
            (Err(e), _) => {
                let _ = debug_println("windowd: fb handoff to gpud send-failed");
                log_gpud_ipc_error("windowd: fb handoff to gpud send-failed detail", e);
                self.gpud_client = None;
                false
            }
            (Ok(()), Err(e)) => {
                let _ = debug_println("windowd: fb handoff to gpud recv-failed");
                log_gpud_ipc_error("windowd: fb handoff to gpud recv-failed detail", e);
                self.gpud_client = None;
                false
            }
        }
    }
}
