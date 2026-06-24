// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the chat window: open/close/move, scroll momentum, and off-screen surface rendering.
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
    /// Wheel over the chat viewport → an inertial **flick**. The signed wheel
    /// delta (real notch count from inputd, no longer a quantized boolean) moves
    /// the virtual-list's scroll *target*; `tick_chat_scroll` then eases the
    /// position toward it over subsequent frames (momentum). One notch animates
    /// smoothly; many notches fling proportionally further — no dropped input.
    pub(super) fn handle_chat_scroll_input(&mut self, wheel_delta_y: i32) {
        if wheel_delta_y == 0 {
            return;
        }
        if !self.scroll_marker_emitted {
            let _ = debug_println(crate::markers::SCROLL_ON_MARKER);
            self.scroll_marker_emitted = true;
        }
        // REL_WHEEL: +up / −down (inputd convention). Scroll offset grows toward
        // the bottom, so a wheel-down (negative delta) increases the offset →
        // negate. Scale each notch to ~3 text lines (the standard wheel step).
        // `wheel_delta_y` here is the COALESCED per-frame total (commit_scroll_input).
        // Clamp it: bounds one frame's scroll AND drops stale piled-up backlog
        // (reactive — apply this frame's intent, not a replayed flood). At ~120 Hz
        // this still allows ~24·120 ≈ 2880 notches/s; the scroller's acceleration
        // handles fast sequences across frames.
        const MAX_NOTCHES_PER_FRAME: i32 = 24;
        let notches = wheel_delta_y.clamp(-MAX_NOTCHES_PER_FRAME, MAX_NOTCHES_PER_FRAME);
        let step = crate::interaction::CHAT_LINE_H.saturating_mul(3) as i32;
        let delta_px = -notches.saturating_mul(step);
        // `scroll_wheel` moves the content IMMEDIATELY (1:1, zero latency — precise
        // for a slow careful scroll) and injects accumulating momentum (a fast
        // spin coasts). Commit the instant move NOW so it presents on this very
        // loop iteration's flush; the momentum tick continues the glide after.
        self.chat_list.scroll_wheel(FxPx::new(delta_px));
        self.commit_chat_scroll_position();
    }

    /// Apply the wheel input coalesced this present-loop iteration — ONCE, with the
    /// frame's net delta — then clear it. Called after draining the IPC batch so a
    /// flood of queued input events becomes a single reactive scroll step instead
    /// of a replayed backlog ("old commands still being processed"). Returns true
    /// if it scrolled (so the caller knows to keep the pacer alive).
    pub(crate) fn commit_scroll_input(&mut self) -> bool {
        let delta = core::mem::take(&mut self.pending_chat_wheel);
        if delta == 0 {
            return false;
        }
        self.handle_chat_scroll_input(delta);
        true
    }

    /// Mirror the virtual-list scroll position into `chat_scroll_y`, recenter the
    /// overscan render base only when the scroll leaves the prerendered window,
    /// and re-present the chat region (a cheap GPU source-row offset, not a CPU
    /// re-render). Shared by the immediate wheel step + the per-frame momentum
    /// tick so both commit identically.
    pub(super) fn commit_chat_scroll_position(&mut self) {
        let new = self.chat_list.scroll_offset().as_i32().max(0) as u32;
        if new == self.chat_scroll_y {
            return;
        }
        self.chat_scroll_y = new;
        let offset = new.saturating_sub(self.chat_render_base);
        let within_overscan = new >= self.chat_render_base && offset <= CHAT_OVERSCAN;
        if within_overscan && self.gl_cursor_active {
            // SCROLL FAST PATH (virgl GL scanout): tell gpud to re-sample the
            // retained chat layer at the new atlas row and GPU-re-composite (~54µs)
            // — NO windowd CPU compose, just like the cursor's OP_MOVE_CURSOR. This
            // is what lets scroll run at gpud's rate instead of windowd's compose rate.
            self.send_chat_scroll_to_gpud(self.chat_atlas_row() + offset);
        } else {
            // Left the prerendered window (new content needed) OR the 2D/mmio path
            // (no GL layer re-sample): recenter + re-render as needed, then a normal
            // present carries the fresh layer (which also clears gpud's scroll override).
            if !within_overscan {
                self.chat_render_base = new.saturating_sub(CHAT_OVERSCAN / 2);
                self.chat.surface_dirty = true;
            }
            self.queue_gpu_blit_rect(DamageRect {
                x: crate::interaction::CHAT_PANEL_X,
                y: crate::interaction::CHAT_PANEL_Y,
                width: crate::interaction::CHAT_PANEL_W,
                height: crate::interaction::CHAT_PANEL_H,
            });
        }
        if !self.live_scroll_marker_emitted {
            let _ = debug_println(crate::markers::LIVE_SCROLL_OK_MARKER);
            self.live_scroll_marker_emitted = true;
        }
    }

    /// Scroll fast path: a 5-byte fire-and-forget `OP_SET_CHAT_SCROLL(src_row)` to
    /// gpud (mirrors the cursor's `OP_MOVE_CURSOR`). gpud re-samples the retained
    /// scrollable chat layer at `src_row_abs` and re-composites on the GPU — no
    /// windowd compose. No-op on the 2D/mmio backend (handled there by the CPU path).
    pub(super) fn send_chat_scroll_to_gpud(&mut self, src_row_abs: u32) {
        let mut frame = [0u8; 5];
        frame[0] = GPU_SET_CHAT_SCROLL_OP;
        frame[1..5].copy_from_slice(&src_row_abs.to_le_bytes());
        let _ = self.send_gpud_fire_forget(&frame);
    }

    /// Advance the chat scroll momentum ONE frame (called from the present-loop
    /// pacing tick with `now_ns`). Integrates `chat_list`'s velocity over the real
    /// elapsed time since the last tick (frame-rate independent), mirrors the new
    /// position into `chat_scroll_y`, recenters the overscan render base only when
    /// the scroll leaves the prerendered window, and re-presents the chat region
    /// (a cheap GPU source-row offset, not a CPU re-render). Returns true while
    /// still gliding so the pacer keeps ticking. This is what makes the live chat
    /// scroll buttery (momentum) instead of a one-shot jump.
    pub(crate) fn tick_chat_scroll(&mut self, now_ns: u64) -> bool {
        if !self.chat_list.is_animating() {
            self.chat_scroll_last_ns = 0;
            return false;
        }
        // Real elapsed time since the last tick; on the first frame of a glide
        // (last == 0) assume one 120 Hz frame so the integrator starts cleanly.
        let dt_ns = if self.chat_scroll_last_ns == 0 || now_ns <= self.chat_scroll_last_ns {
            8_333_333
        } else {
            now_ns - self.chat_scroll_last_ns
        };
        self.chat_scroll_last_ns = now_ns;
        let still = self.chat_list.tick(dt_ns);
        if !still {
            self.chat_scroll_last_ns = 0;
        }
        // GPU scroll-offset: while the scroll stays inside the prerendered overscan
        // window the commit is a pure composite source-row offset (no CPU re-render).
        self.commit_chat_scroll_position();
        still
    }

    /// Toggle the chat window open/closed (shared by the dropdown item + the
    /// proof-panel chat button). On open: rebuild the blur cache (the backdrop may
    /// be stale from a prior position) and damage the window region. On close:
    /// cancel any drag and erase the vacated region.
    pub(super) fn toggle_chat(&mut self) {
        if self.shell_config.locked {
            return; // kiosk lockdown: no launcher windows
        }
        let now = !self.chat.visible;
        self.chat.visible = now;
        if now {
            let _ = debug_println("windowd: chat window open");
            // Surface content is retained in the atlas — just damage the window
            // region (plus shadow halo) so the composite draws it at the current
            // bounds. The blurred-backdrop cache may be stale from a prior pos.
            self.chat.blur_valid = false;
            self.erase_chat_region(self.chat.x, self.chat.y);
            self.note_chat_button_dirty();
        } else {
            self.chat.end_drag();
            self.on_chat_window_closed();
        }
    }

    /// Damage the chat window's last region so the base shows through after the
    /// window is closed (the composite no longer draws it).
    pub(super) fn on_chat_window_closed(&mut self) {
        let _ = debug_println("windowd: chat window close");
        self.erase_chat_region(self.chat.x, self.chat.y);
        self.note_chat_button_dirty();
    }

    /// Damage the chat toggle button's rect so the incremental composite redraws its
    /// active-state tint after a chat-visibility change (its body alpha tracks
    /// `self.chat.visible`). Without this the gated button block would keep the
    /// stale tint until another damage rect happened to touch it.
    pub(super) fn note_chat_button_dirty(&mut self) {
        let cb = crate::interaction::chat_button_rect(self.mode.width, self.mode.height);
        self.queue_gpu_blit_rect(DamageRect {
            x: cb.x,
            y: cb.y,
            width: cb.width,
            height: cb.height,
        });
    }

    /// A drag moved the chat window: erase the old region (a cheap GPU blit of
    /// the base from Plane 1 — the chat was never baked there), and the
    /// compositor re-blits the cached chat surface at the new bounds. No CPU
    /// recomposite, no content re-render → GPU-bound drag.
    pub(super) fn note_chat_window_moved(&mut self, old: DamageRect) {
        if !self.chat_drag_marker_emitted {
            let _ = debug_println("windowd: chat window drag ok");
            self.chat_drag_marker_emitted = true;
        }
        // `ShellWindow::drag_to` already invalidated the blur cache (the backdrop
        // behind the window changed). Erase the vacated region (window + shadow
        // halo, already padded by `drag_to`) so the moved window leaves no trail;
        // the composite re-blits the cached chat surface at the new position.
        self.queue_gpu_blit_rect(old);
    }

    /// Refresh the display from the (cursor-free, chat-free) base in Plane 1 for
    /// a chat-sized region at (x, y). Pure GPU blit — no recomposite. The region
    /// is padded by the drop-shadow halo (blur + offset) so a moved window
    /// leaves no stale shadow behind.
    pub(super) fn erase_chat_region(&mut self, x: i32, y: i32) {
        let pad = CHAT_SHADOW_BLUR.saturating_add(CHAT_SHADOW_OFFSET_Y.unsigned_abs()) as i32;
        self.queue_gpu_blit_rect(DamageRect {
            x: (x - pad).max(0) as u32,
            y: (y - pad).max(0) as u32,
            width: crate::interaction::CHAT_PANEL_W + 2 * pad as u32,
            height: crate::interaction::CHAT_PANEL_H + 2 * pad as u32,
        });
    }

    /// Absolute atlas row of the chat content surface (`0` if somehow unmounted —
    /// chat mounts at boot and only hides, never unmounts, so this is always set).
    pub(super) fn chat_atlas_row(&self) -> u32 {
        self.chat.atlas.map(|s| s.abs_row).unwrap_or(0)
    }

    /// Render the full chat layer content into its off-screen atlas surface
    /// (rows `chat.atlas.abs_row..`, x 0..CHAT_PANEL_W). Called when the surface
    /// is dirty (init / scroll / new message), never per move — moving the
    /// window only changes the composite blit destination.
    pub(super) fn render_chat_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let Some(surface) = self.chat.atlas else {
            return Ok(()); // unmounted — nothing to render
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        // Render the OVERSCAN surface (viewport + overscan) anchored at the
        // current render base. Re-window at that base so the surface content
        // matches; scrolling within the overscan is a composite offset (no
        // re-render), so this runs only on init / overscan-exhaustion / new data.
        let height = crate::interaction::CHAT_PANEL_H + CHAT_OVERSCAN;
        let content_vp_h = height
            .saturating_sub(crate::interaction::CHAT_TITLE_BAR_H + crate::interaction::CHAT_PAD);
        self.chat_content_h = super::chat::compute_visible_window(
            self.chat_list.provider(),
            self.chat_render_base,
            &mut self.chat_visible,
            content_vp_h,
        );
        // Keep the component's height authority in sync so momentum clamps to the
        // real bottom (re-render happens on data change / overscan exhaustion).
        self.chat_list.set_content_height(FxPx::new(self.chat_content_h as i32));
        let abs_row = surface.abs_row;
        let render_base = self.chat_render_base;
        let content_h = self.chat_content_h;
        let visible = &self.chat_visible;
        let band = &mut self.band_scratch;
        // Write the surface in ROW_WRITE_CHUNK-row bands: one vmo_write syscall
        // per band instead of one per row. The band carries full-stride rows; the
        // chat draws into x<366 and the unused atlas padding is never sampled.
        let mut band_start = 0u32;
        while band_start < height {
            let band_end = (band_start + ROW_WRITE_CHUNK as u32).min(height);
            let band_rows = (band_end - band_start) as usize;
            for (i, ly) in (band_start..band_end).enumerate() {
                let row = &mut band[i * stride..(i + 1) * stride];
                super::chat::draw_chat_panel_row(ly, row, render_base, content_h, visible, height)?;
            }
            let dst = (abs_row + band_start) as usize * stride;
            vmo_write(handle, dst, &band[..band_rows * stride])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }
}
