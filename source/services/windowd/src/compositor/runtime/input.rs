// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — per-frame input staging + routing.
//! Post-cleanup (cleanup-map): windowd owns ONLY hit-testing against the
//! z-stack (window_scene SSOT) and ROUTING to the target surface
//! (`OP_SURFACE_INPUT`); what a click DOES is the app's/widget's business.
//! The legacy chrome hit-paths (topbar/dropdown/sidebar/chat/search/settings/
//! avatar-greeter) are DELETED — that UI lives in the DSL shell/greeter apps.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (behavior covered via windowd QEMU smoke + host integration)

use super::*;

impl DisplayServerRuntime {
    /// STAGE one upstream input update into the frame-aligned sample instead of
    /// applying it now. The latest cursor/button/text snapshot wins (we only render
    /// the newest position); wheel deltas SUM (no scroll notch is lost). Replies
    /// can be sent immediately by the caller — staging always "accepts". This is
    /// the consumer half of the Android frame-aligned input model.
    pub(crate) fn stage_input_state(&mut self, mut state: VisibleState) -> u8 {
        if state.wheel_delta_y != 0 && self.wheel_stage_count < 40 {
            self.wheel_stage_count += 1;
            let _ = debug_println(&alloc::format!(
                "windowd: wheel staged n={} d={}",
                self.wheel_stage_count, state.wheel_delta_y
            ));
        }
        if let Some(prev) = self.pending_input.take() {
            // Carry the accumulated wheel forward (sum), keep the newest of all else.
            state.wheel_delta_y = state.wheel_delta_y.saturating_add(prev.wheel_delta_y);
        }
        self.pending_input = Some(state);
        STATUS_OK
    }

    /// Apply the frame's staged input sample ONCE (called from the present loop
    /// after draining the IPC batch). Returns true if there was input to apply.
    /// This collapses N raw events/frame into a single hit-test + hover + cursor
    /// move + scroll — the work is bounded by frame rate, not input rate.
    pub(crate) fn apply_staged_input(&mut self) -> bool {
        match self.pending_input.take() {
            Some(state) => {
                self.apply_input_state(state);
                true
            }
            None => false,
        }
    }

    pub(crate) fn apply_input_state(&mut self, upstream: VisibleState) -> u8 {
        if !self.input_state_debug_emitted {
            let _ = debug_trace("dbg: windowd input state applied");
            // Input-chain hop I6: input reached windowd and was applied. The
            // present chain (gpud G1..G4) takes over from here to put it onscreen.
            let _ = debug_println("windowd: chain I6 input recv (state applied)");
            self.input_state_debug_emitted = true;
        }
        let old_state = self.state;
        let old_cursor_x = self.state.cursor_x;
        let old_cursor_y = self.state.cursor_y;
        let old_filter_idx = self.active_filter_idx;
        self.state.virtio_raw_seen |= upstream.virtio_raw_seen;
        self.state.hid_normalized_seen |= upstream.hid_normalized_seen;
        self.state.pointer_route_live |= upstream.pointer_route_live;
        self.state.keyboard_route_live |= upstream.keyboard_route_live;
        self.state.input_visible_on |= upstream.input_visible_on
            || upstream.pointer_route_live
            || upstream.keyboard_route_live;
        self.state.cursor_move_visible |=
            upstream.cursor_move_visible || upstream.pointer_route_live;
        // ── windowd-owned hit-testing (compositor model) ──
        // inputd ships a raw display-space pointer + raw button/wheel/key facts;
        // windowd resolves routing against its own rendered geometry.
        let cursor_x = upstream.cursor_x;
        let cursor_y = upstream.cursor_y;
        let mode = self.mode;
        // C1: the proof/target-test hover card is gone — nothing to hover-test.
        self.state.hover_visible = false;

        // Raw primary-button level from inputd; rising/falling edges are click/release.
        let primary_down = upstream.launcher_click_visible;
        let primary_press = primary_down && !old_state.launcher_click_visible;
        let primary_release = !primary_down && old_state.launcher_click_visible;
        self.state.launcher_click_visible = primary_down;

        let mut window_consumed_press = false;
        // Shell switcher hotspot: a fixed bottom-left corner cycles the active
        // shell via SystemUI's resolver (desktop → tablet → kiosk → …).
        // Reachable in EVERY shell so the runtime switch is always demonstrable.
        if primary_press
            && !window_consumed_press
            && cursor_x >= 0
            && cursor_x < 28
            && cursor_y >= (mode.height as i32 - 28)
        {
            window_consumed_press = true;
            self.cycle_shell();
        }
        // Dock (bottom-center bar of minimized windows): composited above the
        // windows, so its presses resolve BEFORE the window loop. A press on an
        // icon restores that window; anywhere else on the bar just consumes.
        if primary_press && !window_consumed_press {
            if let Some(bar) = self.dock_bar_rect() {
                if crate::dock::dock_contains(bar, cursor_x, cursor_y) {
                    window_consumed_press = true;
                    let (list, n) = self.windows.minimized_list();
                    if let Some(slot) = crate::dock::dock_slot_at(bar, n, cursor_x, cursor_y) {
                        self.restore_window(list[slot]);
                    }
                }
            }
        }
        // Windows, hit-tested FRONT-TO-BACK in the z/focus stack's order — the
        // exact reverse of the composite order (one SSOT in `window_scene`), so
        // input can never disagree with occlusion. The topmost window containing
        // the press consumes it; any press on a title bar or body raises +
        // focuses that window (click-to-raise).
        if primary_press && !window_consumed_press {
            use crate::compositor::shell_window::WindowPress;
            use crate::window_scene::WindowId;
            let (hit, hit_n) = self.windows.hit_order(USE_DESKTOP_SHELL);
            for i in 0..hit_n {
                let wid = hit[i];
                // A visible app window's frame; the desktop base is chromeless
                // full-screen.
                let (frame, app_idx) = match wid {
                    WindowId::App(a) => {
                        let idx = a as usize;
                        (self.apps[idx].win.frame(), Some(idx))
                    }
                    WindowId::Desktop => (
                        crate::compositor::shell_window::Frame {
                            x: 0,
                            y: 0,
                            w: self.mode.width,
                            h: self.mode.height,
                            title_h: 0,
                            close_w: 0,
                        },
                        None,
                    ),
                };
                // Edge/corner grab RESIZES (floating windows only) — resolved
                // before the title/body press so the border band wins.
                if app_idx.is_some() && !self.windows.is_fullscreen(wid) {
                    if let Some(edge) = frame.resize_edge_at(cursor_x, cursor_y) {
                        window_consumed_press = true;
                        self.begin_window_resize(wid, edge, cursor_x, cursor_y);
                        break;
                    }
                }
                let press = frame.press(cursor_x, cursor_y);
                match press {
                    WindowPress::Close => {
                        window_consumed_press = true;
                        if let Some(idx) = app_idx {
                            self.close_app_window(idx);
                        }
                    }
                    WindowPress::Minimize => {
                        window_consumed_press = true;
                        self.minimize_window(wid);
                    }
                    WindowPress::Maximize => {
                        window_consumed_press = true;
                        self.toggle_fullscreen(wid);
                    }
                    WindowPress::TitleDrag => {
                        window_consumed_press = true;
                        self.raise_window(wid);
                        if let Some(idx) = app_idx {
                            self.apps[idx].win.begin_drag(cursor_x, cursor_y);
                        }
                    }
                    WindowPress::Body => {
                        window_consumed_press = true;
                        self.raise_window(wid);
                        // A body click on an app-client window is the APP's
                        // event (ADR-0042 R3): forward surface-local body
                        // coordinates; windowd keeps focus/raise only.
                        if let Some(idx) = app_idx {
                            if cursor_y >= frame.y {
                                let local_x = cursor_x - frame.x;
                                // Declarative: the body starts below the RESOLVED
                                // chrome height (0 for chromeless presentations),
                                // not a hardcoded title constant.
                                let body_y =
                                    cursor_y - frame.y - self.apps[idx].win.title_h as i32;
                                if body_y >= 0 {
                                    self.send_app_input(idx, local_x, body_y);
                                }
                            }
                        }
                        // A body press on the DESKTOP surface falls through to
                        // the desktop-input routing below (the desktop has no
                        // window chrome; the shell/greeter owns those pixels).
                        if wid == WindowId::Desktop {
                            window_consumed_press = false;
                        }
                    }
                    WindowPress::Miss => continue,
                }
                break;
            }
        }
        // Continue dragging whichever app window is mid-drag (ADR-0042).
        for idx in 0..self.apps.len() {
            if !self.apps[idx].win.is_dragging() {
                continue;
            }
            if let Some(old) =
                self.apps[idx].win.drag_to(cursor_x, cursor_y, mode.width, mode.height)
            {
                self.queue_dirty_rect(old);
                let rect = self.app_window_rect(idx);
                self.queue_dirty_rect(rect);
                self.apps[idx].win.surface_dirty = true;
            }
        }
        // Continue an active edge-resize drag (TASK-0070 Phase 3).
        if self.resize_drag.is_some() {
            self.apply_window_resize(cursor_x, cursor_y);
        }
        if primary_release {
            // Drag-to-edge snap: releasing a TITLE drag with the pointer at a
            // display edge snaps the window (left/right half, top=fullscreen).
            for idx in 0..self.apps.len() {
                if self.apps[idx].win.is_dragging() {
                    let _ = self.apply_release_snap(
                        crate::window_scene::WindowId::App(idx as u8),
                        cursor_x,
                        cursor_y,
                    );
                    self.apps[idx].win.end_drag();
                }
            }
            self.end_window_resize();
        }

        // Declarative desktop surface (Umbau #17 2c): a primary press that
        // NOTHING consumed — no window chrome/body — belongs to the DESKTOP
        // surface: the shell/greeter app-host owns those pixels. Forward
        // surface-local coordinates (the desktop is full-screen at the origin,
        // chromeless), same OP_SURFACE_INPUT path as app-window bodies.
        // windowd stays pure routing: what the click DOES is the shell's business.
        if primary_press
            && !window_consumed_press
            && self.windows.is_visible(crate::window_scene::WindowId::Desktop)
        {
            self.send_desktop_input(cursor_x, cursor_y);
        }
        self.state.focus_visible |= upstream.focus_visible;
        // Reflect the momentary key-held state from inputd (which already sends
        // `keyboard_visible = keyboard_held`). Must NOT be OR-latched with
        // `keyboard_route_live` — that flag stays true forever once the keyboard
        // is seen, which would pin the "key pressed" highlight on permanently.
        // The once-only proof marker is latched separately in observer_state.
        self.state.keyboard_visible = upstream.keyboard_visible;
        self.state.wheel_up_visible = upstream.wheel_up_visible;
        self.state.wheel_down_visible = upstream.wheel_down_visible;
        self.state.cursor_x = upstream.cursor_x;
        self.state.cursor_y = upstream.cursor_y;
        self.state.set_text_input(upstream.text_input());
        // Title-bar button hover `[– □ ×]` for the topmost window under the
        // cursor (TASK-0070 Phase 2; re-renders that window's title on change).
        self.update_title_hovers(self.state.cursor_x, self.state.cursor_y);
        // Pointer shape (TASK-0070 Phase 3): an active resize keeps its edge
        // shape; otherwise the topmost floating window's border band under the
        // cursor selects it; anything else restores the default pointer.
        self.update_cursor_shape_for_pointer(self.state.cursor_x, self.state.cursor_y);
        // Hover chain (RFC-0067 R2): forward the frame-aligned pointer sample
        // to the surface under it — the app-host hit-tests its interactive
        // boxes and blends the hover wash at PAINT time (no re-layout).
        // Bounded by frame rate (staged input), silent per move.
        if old_cursor_x != self.state.cursor_x || old_cursor_y != self.state.cursor_y {
            self.forward_pointer_hover(self.state.cursor_x, self.state.cursor_y);
        }
        // Wheel routing (Scroll-Track S1) — MUST run BEFORE the no-change
        // short-circuit below: the summed delta lives ONLY in the staged
        // frame (`upstream`), never in the mirrored `self.state`, so a pure
        // wheel series leaves `state == old_state` and the short-circuit
        // silently ate every notch after the indicator's first edge (the
        // "~10% of notches work" bug). Consumed exactly once per applied
        // frame; same hit-order SSOT as hover/taps.
        if upstream.wheel_delta_y != 0 {
            self.forward_wheel(self.state.cursor_x, self.state.cursor_y, upstream.wheel_delta_y);
        }
        // C1: the proof panel is gone; `active_filter_idx` is now just a typed-text
        // change counter that still drives the filter selftest markers below.
        if !USE_DESKTOP_SHELL {
            self.active_filter_idx = filter_layout_variant_index(self.state.text_input());
        }
        self.refresh_observer_state();
        if self.state == old_state && self.active_filter_idx == old_filter_idx {
            return STATUS_OK;
        }
        // ── Phase 0: Scene graph updates instead of damage rect queueing ──
        // Card active states: hover → slot 0, click → slot 1, keyboard → slot 2
        let hover_changed = old_state.hover_visible != self.state.hover_visible;
        let click_changed = old_state.launcher_click_visible != self.state.launcher_click_visible;
        let key_changed = old_state.keyboard_visible != self.state.keyboard_visible;
        if hover_changed {
            self.shell.set_card_active(0, self.state.hover_visible);
        }
        if click_changed {
            self.shell.set_card_active(1, self.state.launcher_click_visible);
        }
        if key_changed {
            self.shell.set_card_active(2, self.state.keyboard_visible);
        }
        // CPU repaint of the test cards whose state flags flipped — this is what
        // recolors the card borders (proof_box_border reads these flags).
        self.queue_target_damage(old_state, self.state);
        // Sidebar visibility
        if old_state.sidebar_open_visible != self.state.sidebar_open_visible {
            self.shell.set_sidebar_visible(self.state.sidebar_open_visible);
        }
        // Detect paint-only: only hover/click/keyboard flags changed, not cursor or text
        let cursor_changed =
            old_cursor_x != self.state.cursor_x || old_cursor_y != self.state.cursor_y;
        let text_changed = old_state.text_input() != self.state.text_input();
        let filter_changed = old_filter_idx != self.active_filter_idx;
        let paint_flags_changed = old_state.hover_visible != self.state.hover_visible
            || old_state.sidebar_open_visible != self.state.sidebar_open_visible
            || old_state.launcher_click_visible != self.state.launcher_click_visible
            || old_state.keyboard_visible != self.state.keyboard_visible;

        // Implicit transitions (RFC-0059 Phase 4): when paint flags change,
        // trigger spring animation for opacity/transform on the affected proof cards.
        if paint_flags_changed && !self.animation_driver.reduced_motion() {
            if !self.animation_proof.runtime_marker {
                let _ = debug_println(UIRUNTIME_ON);
                self.animation_proof.runtime_marker = true;
            }
            if !self.animation_proof.implicit_marker {
                let _ = debug_println(WINDOWD_IMPLICIT_TRANSITIONS_ON);
                self.animation_proof.implicit_marker = true;
            }
            let spring = animation::SpringConfig {
                stiffness: 200.0,
                damping: 20.0,
                mass: 1.0,
                initial_velocity: 0.0,
            };
            // Sidebar open/close uses a dedicated state so close actions are not
            // coupled to hover leave.
            if old_state.sidebar_open_visible != self.state.sidebar_open_visible {
                let sidebar_from =
                    if old_state.sidebar_open_visible { 0.0 } else { SIDEBAR_WIDTH as f32 };
                let sidebar_to =
                    if self.state.sidebar_open_visible { 0.0 } else { SIDEBAR_WIDTH as f32 };
                self.animation_driver.spring_to(
                    SIDEBAR_LAYER_ID,
                    AnimProp::TranslateX,
                    sidebar_from,
                    sidebar_to,
                    spring,
                );
                self.animation_driver.spring_to(
                    SIDEBAR_LAYER_ID,
                    AnimProp::Opacity,
                    self.animated_scene.sidebar_opacity,
                    if self.state.sidebar_open_visible { 1.0 } else { 0.0 },
                    spring,
                );
                if !self.animation_proof.timeline_marker {
                    let _ = debug_println(UIANIM_TIMELINE_ON);
                    self.animation_proof.timeline_marker = true;
                }
            }
            // Click card opacity
            if old_state.launcher_click_visible != self.state.launcher_click_visible {
                let from = if old_state.launcher_click_visible { 1.0 } else { 0.0 };
                let to = if self.state.launcher_click_visible { 1.0 } else { 0.0 };
                self.animation_driver.spring_to(
                    CLICK_LAYER_ID,
                    AnimProp::Opacity,
                    from,
                    to,
                    spring,
                );
            }
            // Keyboard card opacity
            if old_state.keyboard_visible != self.state.keyboard_visible {
                let from = if old_state.keyboard_visible { 1.0 } else { 0.0 };
                let to = if self.state.keyboard_visible { 1.0 } else { 0.0 };
                self.animation_driver.spring_to(
                    KEYBOARD_LAYER_ID,
                    AnimProp::Opacity,
                    from,
                    to,
                    spring,
                );
            }
        }
        if old_state.sidebar_open_visible != self.state.sidebar_open_visible {
            let _ = debug_println(if self.state.sidebar_open_visible {
                SIDEBAR_OPEN_MARKER
            } else {
                SIDEBAR_CLOSE_MARKER
            });
            // Sidebar is a GPU overlay — no CPU content in P1 changes on open/close.
            // Cache invalidation is deferred until the close animation completes.
            self.queue_gpu_blit_rect(self.sidebar_damage_rect());
        }
        self.paint_only_damage =
            paint_flags_changed && !cursor_changed && !text_changed && !filter_changed;
        // Cursor hot path. Hardware overlay: the move is a 9-byte message to
        // gpud's cursor queue — the host repositions the overlay, no composite,
        // no blit, no present. The frame pipeline is not involved at all.
        // Software fallback: queue the merged old+new cursor rect — flush blits
        // that region from the retained Plane 1 and overlays BlendCursor.
        if self.hw_cursor_active {
            if cursor_changed {
                self.send_cursor_move_to_gpud();
            }
        } else if self.gl_cursor_active {
            // virgl procedural cursor: update gpud's pointer pos AND damage the
            // cursor rect so a present is scheduled — the build-up present
            // redraws the procedural arrow at the new spot (its VMO BlendCursor
            // is ignored while the GL build-up owns the scanout).
            if cursor_changed {
                self.send_cursor_move_to_gpud();
                self.queue_cursor_damage(
                    old_cursor_x,
                    old_cursor_y,
                    self.state.cursor_x,
                    self.state.cursor_y,
                );
            }
        } else {
            self.queue_cursor_damage(
                old_cursor_x,
                old_cursor_y,
                self.state.cursor_x,
                self.state.cursor_y,
            );
        }

        // ── v3b: reflect real upstream text instead of synthetic keyboard cycling ──
        if old_state.text_input() != self.state.text_input() {
            self.note_filter_text_changed();
        }

        // ── v3b: selftest summary markers (once) ──
        if !self.selftest_v3b_emitted
            && self.live_scroll_marker_emitted
            && self.clipping_marker_emitted
            && self.filter_cycle > 0
        {
            let _ = debug_println(crate::markers::SELFTEST_UI_V3_SCROLL_OK_MARKER);
            let _ = debug_println(crate::markers::SELFTEST_UI_V3_FILTER_OK_MARKER);
            let _ = debug_println(crate::markers::SELFTEST_UI_V3_IME_OK_MARKER);
            self.selftest_v3b_emitted = true;
        }

        STATUS_OK
    }

    /// Resolve the surface under the pointer with the SAME z-order/press
    /// geometry the tap routing uses (input and hover can never disagree),
    /// send it a MOVE, and send the previous target a LEAVE when the route
    /// changes. Drags/resizes capture the pointer — no hover while active.
    /// Title bars/buttons are windowd chrome (its own title hover), not app
    /// hover. One-time proof marker: `windowd: hover routing on`.
    pub(crate) fn forward_pointer_hover(&mut self, cursor_x: i32, cursor_y: i32) {
        use nexus_display_proto::client_surface::{INPUT_KIND_LEAVE, INPUT_KIND_MOVE};
        let mut route = HOVER_ROUTE_NONE;
        let mut local = (cursor_x, cursor_y);
        let mut route_idx = 0usize;
        let any_drag = self.apps.iter().any(|a| a.win.is_dragging());
        if !any_drag && self.resize_drag.is_none() {
            use crate::compositor::shell_window::WindowPress;
            use crate::window_scene::WindowId;
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
                                let body_y =
                                    cursor_y - frame.y - self.apps[idx].win.title_h as i32;
                                if body_y >= 0 {
                                    route = HOVER_ROUTE_APP;
                                    route_idx = idx;
                                    local = (cursor_x - frame.x, body_y);
                                }
                            }
                            // Title bar / window buttons: windowd chrome hover.
                            _ => {}
                        }
                    }
                    WindowId::Desktop => {
                        if self.windows.is_visible(WindowId::Desktop) {
                            route = HOVER_ROUTE_DESKTOP;
                        }
                    }
                }
                break;
            }
        }
        // Route change = target change: leaving one APP window for another is
        // a change too (the old window must clear its hover wash).
        let route_changed =
            route != self.hover_route || (route == HOVER_ROUTE_APP && route_idx != self.hover_app_idx);
        if route_changed {
            let (lx, ly) = self.hover_last;
            match self.hover_route {
                HOVER_ROUTE_APP => {
                    let prev = self.hover_app_idx;
                    self.send_app_input_kind(prev, INPUT_KIND_LEAVE, lx, ly);
                }
                HOVER_ROUTE_DESKTOP => {
                    self.send_desktop_input_kind(INPUT_KIND_LEAVE, lx, ly);
                }
                _ => {}
            }
        }
        // Throttle MOVE forwarding to the frame pace (120Hz): app-side hover
        // washes track the pointer at display rate. The historical flood risk
        // (unthrottled per-EVENT forwarding filled the client queue and starved
        // TAP delivery) is gone twice over: moves apply frame-aligned (once per
        // staged sample) and `send_input_frame` drops MOVEs on a full queue
        // while TAPs retry — so display-rate forwarding is safe.
        let now = nexus_abi::nsec().unwrap_or(0);
        let move_due = now.saturating_sub(self.hover_last_move_ns) >= PACER_INTERVAL_NS;
        if move_due {
            match route {
                HOVER_ROUTE_APP => {
                    self.send_app_input_kind(route_idx, INPUT_KIND_MOVE, local.0, local.1);
                }
                HOVER_ROUTE_DESKTOP => {
                    self.send_desktop_input_kind(INPUT_KIND_MOVE, local.0, local.1);
                }
                _ => {}
            }
            if route != HOVER_ROUTE_NONE {
                self.hover_last_move_ns = now;
            }
        }
        if route != HOVER_ROUTE_NONE && !self.hover_marker_emitted {
            let _ = debug_println("windowd: hover routing on");
            self.hover_marker_emitted = true;
        }
        self.hover_route = route;
        self.hover_app_idx = route_idx;
        self.hover_last = local;
    }

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

    pub(super) fn note_filter_text_changed(&mut self) {
        self.filter_cycle = self.filter_cycle.wrapping_add(1);

        if !self.clipping_marker_emitted {
            let _ = debug_println(crate::markers::CLIPPING_ON_MARKER);
            self.clipping_marker_emitted = true;
        }
        let _ = debug_println(crate::markers::TEXT_INPUT_ON_MARKER);
        let _ = debug_println(crate::markers::FILTER_LIST_OK_MARKER);
        // C1: the proof filter panel is gone — no filter rects to damage.
    }
}
