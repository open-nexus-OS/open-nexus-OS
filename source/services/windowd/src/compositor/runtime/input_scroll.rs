// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the WebRender compositor-scroll slice
//! of input routing (a pure move out of `input.rs`, no behavior change).
//! windowd is the SINGLE scroll writer for a scrollable app body: a wheel notch
//! feeds the per-slot `ScrollMomentum`, shifts the gpud layer `src_row`
//! (`OP_SET_LAYER_SCROLL`) and pushes the app an absolute `INPUT_KIND_SCROLL_POS`
//! (no per-notch app re-render); flings advance on the pacer tick. Wheel HIT
//! routing (topmost window wins) lives here; the frame-aligned input staging +
//! hover/tap routing stays in `input.rs`.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable

use super::*;

impl DisplayServerRuntime {
    /// Route a wheel delta to the surface under the pointer (the TOPMOST
    /// window wins — the retired `resolve_wheel_target`'s contract, now on
    /// the one z-stack SSOT). Sent as `OP_SURFACE_INPUT` kind=WHEEL with the
    /// signed delta riding the `y` field (`wheel_delta_to_wire`). Single-shot
    /// NONBLOCK like MOVE: dropping wheel under queue pressure is correct —
    /// the next notch re-derives the position. Alloc-free.
    pub(crate) fn forward_wheel(&mut self, cursor_x: i32, cursor_y: i32, delta_y: i32) {
        use crate::compositor::shell_window::WindowPress;
        use crate::window_scene::WindowId;
        use nexus_display_proto::client_surface as wire;
        let wire_delta = wire::wheel_delta_to_wire(delta_y);
        if self.wheel_route_count < 40 {
            self.wheel_route_count += 1;
            let _ = debug_println(&alloc::format!(
                "windowd: wheel fwd n={} d={delta_y}",
                self.wheel_route_count
            ));
        }
        let (hit, hit_n) = self.windows.hit_order(USE_DESKTOP_SHELL);
        for i in 0..hit_n {
            let wid = hit[i];
            match wid {
                WindowId::App(a) => {
                    let idx = a as usize;
                    let frame = self.apps[idx].win.frame();
                    match frame.press(cursor_x, cursor_y) {
                        WindowPress::Miss => continue,
                        WindowPress::Body => {
                            // WebRender compositor-scroll: a scrollable body is
                            // owned by windowd — the notch shifts the gpud layer
                            // `src_row` (`OP_SET_LAYER_SCROLL`) and pushes the app
                            // an absolute scroll position (NO per-notch re-render).
                            // A non-scroll body forwards the raw wheel as before.
                            if self.apps[idx].scroll_id != 0 {
                                self.forward_body_scroll(idx, delta_y);
                            } else {
                                let local_x = (cursor_x - frame.x).max(0) as u16;
                                self.send_app_wheel(idx, local_x, wire_delta);
                            }
                        }
                        // Title bar / buttons: chrome, not app scroll.
                        _ => {}
                    }
                }
                WindowId::Desktop => {
                    if self.windows.is_visible(WindowId::Desktop) {
                        self.send_desktop_wheel(cursor_x.max(0) as u16, wire_delta);
                    }
                }
            }
            break;
        }
    }

    /// Visible scroll viewport height (rows) of a scrollable app window: the
    /// frame height minus the WM title bar and the app's fixed header + footer.
    fn visible_body_h(&self, idx: usize) -> u32 {
        self.apps[idx].win.h.saturating_sub(
            self.apps[idx]
                .win
                .title_h
                .saturating_add(self.apps[idx].header_h)
                .saturating_add(self.apps[idx].footer_h),
        )
    }

    /// The max absolute scroll offset (rows) for a scrollable app window:
    /// `content_h - visible_body_h`, clamped at 0 (content shorter than the
    /// viewport ⇒ nothing scrolls).
    fn max_scroll_rows(&self, idx: usize) -> u32 {
        self.apps[idx].content_h.saturating_sub(self.visible_body_h(idx))
    }

    /// A wheel notch over a compositor-scrolled window body (Scroll-Track /
    /// WebRender): feed the notch into the per-slot `ScrollMomentum`, apply the
    /// eased first step, and (a) emit `OP_SET_LAYER_SCROLL` so gpud re-samples
    /// the body at the new `src_row`, (b) push the app an absolute
    /// `INPUT_KIND_SCROLL_POS` so its hit-test / EndReached stay in sync WITHOUT
    /// a re-render. windowd is the SINGLE scroll writer; the app never sees
    /// `INPUT_KIND_WHEEL` for a scrollable body. Flings advance in
    /// `advance_app_scrolls` on the pacer tick.
    pub(crate) fn forward_body_scroll(&mut self, idx: usize, delta_notches: i32) {
        let max_rows = self.max_scroll_rows(idx);
        if max_rows == 0 {
            return; // content fits the viewport — nothing to scroll
        }
        // Direct wheel scroll: accumulate the offset and apply it immediately so
        // the gpud `src_row` shift is 1:1 responsive per notch. gpud re-composites
        // its retained layers on each `OP_SET_LAYER_SCROLL` (the app stays out of
        // the loop). Linux REL_WHEEL: +1 = wheel UP (away) ⇒ wheel DOWN grows the
        // offset (content moves up), hence the inversion. (A pacer-eased fling
        // coast is a follow-up polish; the direct path is reliable and snappy.)
        let delta = -delta_notches * SCROLL_STEP_PX;
        let pos = (self.apps[idx].scroll_rows as i32 + delta).clamp(0, max_rows as i32) as u32;
        self.apply_scroll_rows(idx, pos);
    }

    /// Set the window's absolute scroll offset (rows) and publish it: emit
    /// `OP_SET_LAYER_SCROLL` to gpud (the src_row shift) and push
    /// `INPUT_KIND_SCROLL_POS` to the owning app. The gpud override row equals
    /// the body layer's full-present `src_row_abs` (`atlas_row + title_h +
    /// header_h + footer_h + scroll_rows`) so a full present mid-scroll agrees.
    pub(crate) fn apply_scroll_rows(&mut self, idx: usize, scroll_rows: u32) {
        self.apps[idx].scroll_rows = scroll_rows;
        // Emit the gpud src_row override (fire-and-forget 9-byte frame).
        if let Some(surface) = self.apps[idx].win.atlas {
            let src_row = surface
                .abs_row
                .saturating_add(self.apps[idx].win.title_h)
                .saturating_add(self.apps[idx].header_h)
                .saturating_add(self.apps[idx].footer_h)
                .saturating_add(scroll_rows);
            let scroll_id = self.apps[idx].scroll_id;
            let mut frame = [0u8; 9];
            frame[0] = GPU_SET_LAYER_SCROLL_OP;
            frame[1..5].copy_from_slice(&scroll_id.to_le_bytes());
            frame[5..9].copy_from_slice(&src_row.to_le_bytes());
            let _ = self.send_gpud_fire_forget(&frame);
        }
        // Push the absolute scroll position to the app (hit-test / EndReached).
        // NO windowd present is queued here — gpud's `OP_SET_LAYER_SCROLL` fast
        // path re-composites its RETAINED layer set with the new `src_row`
        // (that is the payoff: a scroll frame is a pure GPU re-composite, not an
        // app re-render + windowd re-present). A later full present (window move,
        // other damage) snapshots the current `scroll_rows` into the body layer's
        // `src_row_abs`, so the two paths always agree (no snap-to-top).
        self.push_scroll_pos(idx, scroll_rows);
    }

    /// Push `INPUT_KIND_SCROLL_POS` (absolute scroll rows in the `y` field) to
    /// the app on its dedicated event channel — the app mirrors it into its
    /// `scroll_y` for hit-testing + the EndReached lazy-load check, WITHOUT
    /// re-rendering on every notch.
    fn push_scroll_pos(&mut self, idx: usize, scroll_rows: u32) {
        let Some(id) = self.apps[idx].surface_id else { return };
        let y = scroll_rows.min(u16::MAX as u32) as u16;
        let frame = nexus_display_proto::client_surface::encode_surface_input(
            id,
            nexus_display_proto::client_surface::INPUT_KIND_SCROLL_POS,
            0,
            y,
        );
        let _ = self.send_app_frame(idx, &frame);
    }

    /// True while any scrollable app window's fling is still easing/coasting —
    /// keeps the compositor pacer armed so `advance_app_scrolls` keeps ticking.
    pub(crate) fn has_scroll_momentum(&self) -> bool {
        self.apps
            .iter()
            .any(|a| a.scroll_id != 0 && a.scroll_momentum.is_animating())
    }

    /// Advance every scrollable window's scroll physics by real elapsed time and
    /// re-emit `OP_SET_LAYER_SCROLL` while animating — the fling keeps scrolling
    /// with the app OUT of the per-frame loop (mirrors the frame-pulse cadence).
    pub(crate) fn advance_app_scrolls(&mut self, now_ns: u64) {
        for idx in 0..self.apps.len() {
            if self.apps[idx].scroll_id == 0 || !self.apps[idx].scroll_momentum.is_animating() {
                continue;
            }
            let dt = now_ns
                .saturating_sub(self.apps[idx].scroll_last_ns)
                .min(100_000_000);
            self.apps[idx].scroll_last_ns = now_ns;
            let _ = self.apps[idx].scroll_momentum.tick(dt);
            let max_rows = self.max_scroll_rows(idx);
            let pos = (self.apps[idx].scroll_momentum.offset_px().max(0) as u32).min(max_rows);
            if pos != self.apps[idx].scroll_rows {
                self.apply_scroll_rows(idx, pos);
            }
        }
    }
}
