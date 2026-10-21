// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the compositor-scroll slice of input
//! routing. windowd is the SINGLE scroll writer for a scrollable app body: a
//! wheel notch feeds the per-slot `ScrollMomentum`, owes a frame whose present
//! carries the body at its new `src_row` (TASK-0338: the present is the only
//! frame) and pushes the app an absolute `INPUT_KIND_SCROLL_POS` (no per-notch
//! app re-render); flings advance on the frame clock. Wheel HIT routing (topmost
//! window wins) lives here; the frame-aligned input staging + hover/tap routing
//! stays in `input.rs`.
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
        let (hit, hit_n) = self.windows.hit_order(self.capture.frozen());
        for i in 0..hit_n {
            let wid = hit[i];
            // The shell's bar, panels and modal take the wheel above any window (`shell_band`).
            if matches!(wid, WindowId::App(_)) && self.shell_owns_point(cursor_x, cursor_y) {
                continue;
            }
            match wid {
                WindowId::App(a) => {
                    let idx = a as usize;
                    let frame = self.apps[idx].win.frame();
                    // TASK-0074 D4: a modal-gated sibling takes no wheel.
                    if matches!(
                        crate::modal_gate::input_verdict(idx, &self.modal_facts()),
                        crate::modal_gate::Verdict::RefusedByModal { .. }
                    ) && !matches!(frame.press(cursor_x, cursor_y), WindowPress::Miss)
                    {
                        break;
                    }
                    match frame.press(cursor_x, cursor_y) {
                        WindowPress::Miss => continue,
                        WindowPress::Body => {
                            // Compositor-scroll: a scrollable body is owned by
                            // windowd — the notch moves the body's `src_row` in the
                            // next present and pushes the app an absolute scroll
                            // position (NO per-notch re-render).
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
    /// frame height minus the app's fixed header + footer.
    fn visible_body_h(&self, idx: usize) -> u32 {
        self.apps[idx]
            .win
            .h
            .saturating_sub(self.apps[idx].header_h.saturating_add(self.apps[idx].footer_h))
    }

    /// The max absolute scroll offset (rows) for a scrollable app window:
    /// `content_h - visible_body_h`, clamped at 0 (content shorter than the
    /// viewport ⇒ nothing scrolls).
    fn max_scroll_rows(&self, idx: usize) -> u32 {
        self.apps[idx].content_h.saturating_sub(self.visible_body_h(idx))
    }

    /// A wheel notch over a compositor-scrolled window body: feed the notch into
    /// the per-slot `ScrollMomentum`, apply the eased first step, and (a) owe a
    /// frame whose present re-samples the body at the new `src_row`, (b) push the
    /// app an absolute `INPUT_KIND_SCROLL_POS` so its hit-test / EndReached stay
    /// in sync WITHOUT a re-render. windowd is the SINGLE scroll writer; the app
    /// never sees `INPUT_KIND_WHEEL` for a scrollable body. Flings advance in
    /// `advance_app_scrolls` on the frame clock.
    pub(crate) fn forward_body_scroll(&mut self, idx: usize, delta_notches: i32) {
        let max_rows = self.max_scroll_rows(idx);
        if max_rows == 0 {
            return; // content fits the viewport — nothing to scroll
        }
        // Eased wheel scroll: the notch extends the momentum TARGET (fixed step
        // per notch, no acceleration); `pos` coasts toward it in
        // `advance_app_scrolls` on the frame clock — each tick a present that
        // moves the body's `src_row`, the app OUT of the loop. Linux REL_WHEEL: +1 = wheel
        // UP (away) ⇒ wheel DOWN grows the offset (content moves up).
        let visible = self.visible_body_h(idx) as f32;
        let content = self.apps[idx].content_h as f32;
        self.apps[idx].scroll_momentum.set_extent(visible, content);
        // Seed the tick clock ONLY when starting from rest. Resetting it on every
        // notch of a continuous burst collapses advance's `dt = now - last` to ~0
        // and starves the ease (it crawled a few px). The momentum owns the dt.
        if !self.apps[idx].scroll_momentum.is_animating() {
            self.apps[idx].scroll_last_ns = nexus_abi::nsec().unwrap_or(0);
        }
        let delta = -delta_notches * SCROLL_STEP_PX;
        let _ = self.apps[idx].scroll_momentum.scroll_wheel(delta as f32);
        // Keep the pacer alive from THIS wheel event onward: apply the eased pos
        // now (may be ~0), and mark that scroll pacing is needed so the coast is
        // never dropped between the wheel and the first pacer tick.
        let pos = (self.apps[idx].scroll_momentum.offset_px().max(0) as u32).min(max_rows);
        self.apply_scroll_rows(idx, pos);
    }

    /// Set the window's absolute scroll offset (rows) and publish it: owe a frame
    /// — its present carries the body layer at `src_row_abs = atlas_row +
    /// header_h + footer_h + scroll_rows` (the scene encode reads `scroll_rows`),
    /// a pure GPU re-composite of the retained layers, no app re-render and no
    /// CPU recomposite — and push `INPUT_KIND_SCROLL_POS` to the owning app.
    /// One frame per completion: a burst of notches coalesces into the latest
    /// row (TASK-0338: the present is the only frame, inside the budget).
    pub(crate) fn apply_scroll_rows(&mut self, idx: usize, scroll_rows: u32) {
        self.apps[idx].scroll_rows = scroll_rows;
        self.owe_frame();
        // Push the absolute scroll position to the app (hit-test / EndReached).
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
    /// keeps the compositor's frame clock running so `advance_app_scrolls` keeps ticking.
    pub(crate) fn has_scroll_momentum(&self) -> bool {
        self.apps.iter().any(|a| a.scroll_id != 0 && a.scroll_momentum.is_animating())
    }

    /// Advance every scrollable window's scroll physics by real elapsed time and
    /// owe a frame per step while animating — the fling keeps scrolling with the
    /// app OUT of the per-frame loop (mirrors the frame-pulse cadence).
    pub(crate) fn advance_app_scrolls(&mut self, now_ns: u64) {
        for idx in 0..self.apps.len() {
            if self.apps[idx].scroll_id == 0 || !self.apps[idx].scroll_momentum.is_animating() {
                continue;
            }
            let dt = now_ns.saturating_sub(self.apps[idx].scroll_last_ns).min(100_000_000);
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
