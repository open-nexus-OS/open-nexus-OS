// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — per-frame input staging + `apply_input_state` routing (hover, filter text, focus/click dispatch).
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
    /// STAGE one upstream input update into the frame-aligned sample instead of
    /// applying it now. The latest cursor/button/text snapshot wins (we only render
    /// the newest position); wheel deltas SUM (no scroll notch is lost). Replies
    /// can be sent immediately by the caller — staging always "accepts". This is
    /// the consumer half of the Android frame-aligned input model.
    pub(crate) fn stage_input_state(&mut self, mut state: VisibleState) -> u8 {
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
        // windowd resolves all UI intent against its own rendered geometry, so a
        // control's hit area is exactly its rendered rect (interaction::*).
        let cursor_x = upstream.cursor_x;
        let cursor_y = upstream.cursor_y;
        let mode = self.mode;

        // Two independent hover signals, both from real rendered geometry:
        //  - hover_visible: cursor over the HOVER TEST CARD in the proof panel
        //    (its border recolors — the actual "hover test"). The card rect
        //    comes from the live layout index, so hit area == rendered rect.
        //  - button_hover: cursor over the top-right glass button (highlight
        //    only; the sidebar animation stays click-driven — user requirement:
        //    "nur der button rechts oben soll die animation auslösen").
        let old_button_hover = self.button_hover;
        self.button_hover = crate::interaction::hover_over_button(mode, cursor_x, cursor_y);
        // C1: the proof/target-test hover card is gone — nothing to hover-test.
        self.state.hover_visible = false;

        // Raw primary-button level from inputd; rising/falling edges are click/release.
        let primary_down = upstream.launcher_click_visible;
        let primary_press = primary_down && !old_state.launcher_click_visible;
        let primary_release = !primary_down && old_state.launcher_click_visible;
        self.state.launcher_click_visible = primary_down;

        // Window manager first: a press on the chat title bar starts a drag, a
        // press on the close button closes it. Both consume the press so it does
        // not also hit the panel/sidebar logic below.
        let mut window_consumed_press = false;
        // Login greeter gate (TASK-0065B): while the greeter owns the display
        // the avatar is the ONLY interactive element — hotspot, windows and
        // chrome below are unreachable (windowd's pre-session launch gating,
        // host-tested in `interaction::resolve_click_session`). Hover feedback
        // tracks every pointer move.
        let greeter_rect = self.greeter_hit_rect();
        if greeter_rect.is_some() || !self.session_resolved() {
            self.update_greeter_hover(cursor_x, cursor_y);
            if primary_press {
                window_consumed_press = true;
                if crate::interaction::resolve_click_session(
                    mode,
                    false,
                    greeter_rect,
                    cursor_x,
                    cursor_y,
                ) == crate::interaction::ClickAction::GreeterUser
                {
                    self.greeter_login_click();
                }
            }
        }
        // Shell switcher hotspot: a fixed bottom-left corner (always wallpaper, no
        // chrome conflict) cycles the active shell via SystemUI's resolver
        // (desktop → tablet → kiosk → …). Reachable in EVERY shell — even a kiosk
        // with no topbar — so the runtime switch is always demonstrable. Checked
        // first so it consumes the press before the windows/chrome below.
        if primary_press
            && !window_consumed_press
            && cursor_x >= 0
            && cursor_x < 28
            && cursor_y >= (mode.height as i32 - 28)
        {
            window_consumed_press = true;
            self.cycle_shell();
        }
        // Shell-P2b: the topbar menu icon (right) toggles the animated side
        // panel — the same scene-graph-driven slide animation as the hamburger.
        if primary_press && !window_consumed_press && self.chrome_composited() {
            use crate::compositor::desktop_layer::{topbar_menu_icon_hit, TOPBAR_MARGIN_X, TOPBAR_TOP};
            if cursor_x >= TOPBAR_MARGIN_X as i32 && cursor_y >= TOPBAR_TOP as i32 {
                let lx = (cursor_x - TOPBAR_MARGIN_X as i32) as u32;
                let ly = (cursor_y - TOPBAR_TOP as i32) as u32;
                if topbar_menu_icon_hit(lx, ly, self.shell_w) {
                    self.state.sidebar_open_visible = !self.state.sidebar_open_visible;
                    window_consumed_press = true;
                    let _ = debug_trace(if self.state.sidebar_open_visible {
                        "dbg: topbar menu -> sidebar OPEN"
                    } else {
                        "dbg: topbar menu -> sidebar CLOSE"
                    });
                }
            }
        }

        // Topbar menu-item click (Apps or Edit) → toggle THAT item's dropdown.
        if primary_press && !window_consumed_press && self.chrome_composited() {
            use crate::compositor::desktop_layer::{
                topbar_item_at, topbar_item_has_menu, TOPBAR_H, TOPBAR_MARGIN_X, TOPBAR_TOP,
            };
            let item = if cursor_y >= TOPBAR_TOP as i32
                && cursor_y < (TOPBAR_TOP + TOPBAR_H) as i32
                && cursor_x >= TOPBAR_MARGIN_X as i32
            {
                topbar_item_at((cursor_x - TOPBAR_MARGIN_X as i32) as u32)
            } else {
                None
            };
            if let Some(item) = item.filter(|&i| topbar_item_has_menu(i)) {
                window_consumed_press = true;
                // Clicking the open menu's item closes it; clicking another item
                // switches the open menu to it.
                self.open_topbar_menu = if self.open_topbar_menu == Some(item) {
                    None
                } else {
                    Some(item)
                };
                let opening = self.open_topbar_menu == Some(item);
                if opening {
                    // Lazily populate the Apps menu from the live registry on first
                    // open (IPC is well past boot here); the Edit menu is static.
                    if item == 0 {
                        self.ensure_app_menu();
                    }
                    self.dropdown_h = self.active_menu().dropdown_full_h();
                    self.dropdown_surface_dirty = true;
                }
                let spring = animation::SpringConfig {
                    stiffness: 240.0,
                    damping: 24.0,
                    mass: 1.0,
                    initial_velocity: 0.0,
                };
                self.animation_driver.spring_to(
                    DROPDOWN_LAYER_ID,
                    AnimProp::Opacity,
                    self.animated_scene.apps_dropdown_progress,
                    if opening { 1.0 } else { 0.0 },
                    spring,
                );
            }
        }

        // Dropdown item clicks: dispatched by the open menu's app/action id.
        if primary_press
            && !window_consumed_press
            && self.chrome_composited()
            && self.open_topbar_menu.is_some()
        {
            use crate::compositor::desktop_layer::{
                menu_item_x, DROPDOWN_W, TOPBAR_H, TOPBAR_MARGIN_X, TOPBAR_TOP,
            };
            let dx = TOPBAR_MARGIN_X + menu_item_x(self.dropdown_item());
            let dy = TOPBAR_TOP + TOPBAR_H + 4;
            if cursor_x >= dx as i32
                && cursor_y >= dy as i32
                && (cursor_x as u32) < dx + DROPDOWN_W
                && (cursor_y as u32) < dy + self.dropdown_h
            {
                if let Some(idx) = self.active_menu().item_at((cursor_y - dy as i32) as u32) {
                    window_consumed_press = true;
                    // Copy the id out first so dispatch arms can take `&mut self`
                    // (the `active_menu` borrow must end before the toggles).
                    let id = self.active_menu().id_at(idx).map(alloc::string::String::from);
                    // Dispatch by the registry app / action id. Known windows map
                    // to their windowd-hosted ShellWindow; any other installed app
                    // is launched via the lifecycle broker.
                    match id.as_deref() {
                        Some("chat") => self.toggle_chat(),
                        Some("settings") => self.toggle_settings(),
                        Some("search") => {
                            // A MINIMIZED search is still `visible` (docked) —
                            // the launcher toggle restores instead of closing.
                            if self.windows.is_minimized(crate::window_scene::WindowId::Search) {
                                self.restore_window(crate::window_scene::WindowId::Search);
                            } else if self.search.visible {
                                self.close_search();
                            } else {
                                self.open_search();
                            }
                        }
                        Some(other) => self.launch_app(other),
                        None => {}
                    }
                }
            }
        }

        // Dock (bottom-center bar of minimized windows): composited above the
        // windows, so its presses resolve BEFORE the window loop. A press on an
        // icon restores that window; anywhere else on the bar just consumes.
        if primary_press && !window_consumed_press {
            if let Some(bar) = self.dock_bar_rect() {
                if crate::dock::dock_contains(bar, cursor_x, cursor_y) {
                    window_consumed_press = true;
                    let (list, n) = self.windows.minimized_list();
                    if let Some(slot) =
                        crate::dock::dock_slot_at(bar, n, cursor_x, cursor_y)
                    {
                        self.restore_window(list[slot]);
                    }
                }
            }
        }
        // Shell windows, hit-tested FRONT-TO-BACK in the z/focus stack's order —
        // the exact reverse of the composite order (one SSOT in `window_scene`),
        // so input can never disagree with occlusion. Checked AFTER the chrome
        // blocks above because chrome now composites ABOVE the windows. The
        // topmost window containing the press consumes it; any press on a title
        // bar or body raises + focuses that window (click-to-raise).
        if primary_press && !window_consumed_press {
            use crate::compositor::shell_window::WindowPress;
            use crate::window_scene::WindowId;
            let (hit, hit_n) = self.windows.hit_order(USE_DESKTOP_SHELL);
            for i in 0..hit_n {
                let wid = hit[i];
                let frame = match wid {
                    WindowId::Chat => self.chat.frame(),
                    WindowId::Search => self.search.frame(),
                    WindowId::Settings => self.settings_win.frame(),
                    WindowId::AppClient => self.app_win.frame(),
                    WindowId::Desktop => crate::compositor::shell_window::Frame {
                        x: 0,
                        y: 0,
                        w: self.mode.width,
                        h: self.mode.height,
                        title_h: 0,
                        close_w: 0,
                    },
                    WindowId::DslDemo => self.dsl_win.frame(),
                };
                // Edge/corner grab RESIZES (floating windows only) — resolved
                // before the title/body press so the border band wins.
                if !self.windows.is_fullscreen(wid) {
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
                        match wid {
                            WindowId::Chat => {
                                self.chat.visible = false;
                                self.on_chat_window_closed();
                            }
                            WindowId::Search => self.close_search(),
                            WindowId::Settings => self.close_settings(),
                            WindowId::DslDemo => self.close_dsl_demo(),
                            WindowId::AppClient => self.close_app_window(),
                            WindowId::Desktop => {} // no close button on the base
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
                        match wid {
                            WindowId::Chat => self.chat.begin_drag(cursor_x, cursor_y),
                            WindowId::Search => self.search.begin_drag(cursor_x, cursor_y),
                            WindowId::Settings => self.settings_win.begin_drag(cursor_x, cursor_y),
                            WindowId::DslDemo => self.dsl_win.begin_drag(cursor_x, cursor_y),
                            WindowId::AppClient => self.app_win.begin_drag(cursor_x, cursor_y),
                            WindowId::Desktop => {} // base is not draggable
                        }
                    }
                    WindowPress::Body => {
                        window_consumed_press = true;
                        self.raise_window(wid);
                        // A click on the Settings "Theme" row toggles light/dark
                        // live (TASK-0072 Phase 9 — immediate-apply, no OK button).
                        // A body click on the DSL window routes through the
                        // interpreter's hit-testing (TASK-0076B).
                        if matches!(wid, WindowId::DslDemo) && cursor_y >= frame.y {
                            let local_x = cursor_x - frame.x;
                            let local_y = cursor_y - frame.y;
                            self.dsl_pointer_body(local_x, local_y);
                        }
                        // A body click on an app-client window is the APP's
                        // event (ADR-0042 R3): forward surface-local body
                        // coordinates; windowd keeps focus/raise only.
                        if matches!(wid, WindowId::AppClient) && cursor_y >= frame.y {
                            let local_x = cursor_x - frame.x;
                            // Declarative: the body starts below the RESOLVED
                            // chrome height (0 for chromeless presentations),
                            // not a hardcoded title constant.
                            let body_y = cursor_y - frame.y - self.app_win.title_h as i32;
                            if body_y >= 0 {
                                self.send_app_input(local_x, body_y);
                            }
                        }
                        if matches!(wid, WindowId::Settings) && cursor_y >= frame.y {
                            use crate::compositor::desktop_layer::{settings_row_at, SETTINGS_ROW_THEME};
                            let local_y = (cursor_y - frame.y) as u32;
                            if settings_row_at(local_y) == Some(SETTINGS_ROW_THEME) {
                                self.toggle_theme();
                            }
                        }
                    }
                    WindowPress::Miss => continue,
                }
                break;
            }
        }
        // Continue an in-progress chat drag: `ShellWindow::drag_to` clamps to the
        // display and invalidates the blur cache; we erase the vacated region
        // (incl. the shadow halo) so a moved window leaves no trail.
        if self.chat.is_dragging() {
            if let Some(old) = self.chat.drag_to(cursor_x, cursor_y, mode.width, mode.height) {
                self.note_chat_window_moved(old);
            }
        }
        // Continue dragging the Search window.
        if self.search.is_dragging() {
            if let Some(old) = self.search.drag_to(cursor_x, cursor_y, mode.width, mode.height) {
                self.queue_dirty_rect(old);
                self.queue_dirty_rect(self.search_window_rect());
            }
        }
        // Continue dragging the Settings window.
        if self.settings_win.is_dragging() {
            if let Some(old) = self.settings_win.drag_to(cursor_x, cursor_y, mode.width, mode.height)
            {
                self.queue_dirty_rect(old);
                self.queue_dirty_rect(self.settings_window_rect());
            }
        }
        // Continue dragging the DSL demo window (was missing entirely — its
        // begin_drag armed but no window followed the pointer, and without
        // the end_drag below it stayed "stuck": user report 2026-07-06).
        if self.dsl_win.is_dragging() {
            if let Some(old) = self.dsl_win.drag_to(cursor_x, cursor_y, mode.width, mode.height) {
                self.queue_dirty_rect(old);
                self.queue_dirty_rect(self.dsl_window_rect());
                self.dsl_win.surface_dirty = true;
            }
        }
        // Continue dragging the app-client window (ADR-0042).
        if self.app_win.is_dragging() {
            if let Some(old) = self.app_win.drag_to(cursor_x, cursor_y, mode.width, mode.height) {
                self.queue_dirty_rect(old);
                self.queue_dirty_rect(self.app_window_rect());
                self.app_win.surface_dirty = true;
            }
        }
        // Continue an active edge-resize drag (TASK-0070 Phase 3).
        if self.resize_drag.is_some() {
            self.apply_window_resize(cursor_x, cursor_y);
        }
        if primary_release {
            // Drag-to-edge snap: releasing a TITLE drag with the pointer at a
            // display edge snaps the window (left/right half, top=fullscreen).
            // Pointer-driven only — there are no snap keyboard shortcuts.
            use crate::window_scene::WindowId as Wid;
            let snap_candidate = if self.chat.is_dragging() {
                Some(Wid::Chat)
            } else if self.search.is_dragging() {
                Some(Wid::Search)
            } else {
                None
            };
            if let Some(wid) = snap_candidate {
                let _ = self.apply_release_snap(wid, cursor_x, cursor_y);
            }
            self.chat.end_drag();
            self.search.end_drag();
            // Settings is a fixed panel — it does not edge-snap, but its drag
            // must still terminate on release (else it stays "stuck" to the cursor).
            self.settings_win.end_drag();
            // Same no-snap release for the DSL demo + app-client windows.
            self.dsl_win.end_drag();
            self.app_win.end_drag();
            self.end_window_resize();
        }

        // Resolve the click against the rendered geometry (only if the window
        // manager did not consume it). The sidebar is the single click-driven
        // animation trigger.
        if primary_press && !window_consumed_press {
            use crate::interaction::{resolve_click, ClickAction};
            let action = resolve_click(mode, self.state.sidebar_open_visible, cursor_x, cursor_y);
            if !matches!(action, ClickAction::None) {
                window_consumed_press = true;
            }
            match action {
                ClickAction::ToggleSidebar => {
                    self.state.sidebar_open_visible = !self.state.sidebar_open_visible;
                }
                ClickAction::CloseSidebar => {
                    self.state.sidebar_open_visible = false;
                }
                ClickAction::ToggleChat => {
                    if !self.chat_button_marker_emitted {
                        let _ = debug_println("windowd: chat button click ok");
                        self.chat_button_marker_emitted = true;
                    }
                    self.toggle_chat();
                }
                ClickAction::FocusPanel => {
                    self.state.focus_visible = true;
                }
                // Unreachable here: greeter presses are consumed above (the
                // greeter gate); kept explicit so the enum stays exhaustive.
                ClickAction::GreeterUser => {}
                ClickAction::None => {}
            }
        }
        // Declarative desktop surface (Umbau #17 2c): a primary press that
        // NOTHING consumed — no window chrome/body, no legacy shell chrome —
        // belongs to the DESKTOP surface: the shell app-host owns those pixels.
        // Forward surface-local coordinates (the desktop is full-screen at the
        // origin, chromeless), same OP_SURFACE_INPUT path as app-window bodies.
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
        // Shell-P2b: topbar hover. Recompute the hovered item from the cursor and,
        // on change, re-render the topbar atlas + damage its band so the present
        // recomposites with the new hover highlight.
        if self.chrome_composited() {
            use crate::compositor::desktop_layer::{
                topbar_item_at, topbar_menu_icon_hit, TOPBAR_H, TOPBAR_MARGIN_X, TOPBAR_TOP,
            };
            let cx = self.state.cursor_x;
            let cy = self.state.cursor_y;
            let in_bar = cy >= TOPBAR_TOP as i32
                && cy < (TOPBAR_TOP + TOPBAR_H) as i32
                && cx >= TOPBAR_MARGIN_X as i32;
            let (new_hover, new_menu_hover) = if in_bar {
                let lx = (cx - TOPBAR_MARGIN_X as i32) as u32;
                let ly = (cy - TOPBAR_TOP as i32) as u32;
                (topbar_item_at(lx), topbar_menu_icon_hit(lx, ly, self.shell_w))
            } else {
                (None, false)
            };
            if new_hover != self.topbar_hover || new_menu_hover != self.topbar_menu_hover {
                self.topbar_hover = new_hover;
                self.topbar_menu_hover = new_menu_hover;
                self.shell_surface_dirty = true;
                self.queue_dirty_rect(DamageRect {
                    x: TOPBAR_MARGIN_X,
                    y: TOPBAR_TOP,
                    width: self.shell_w,
                    height: TOPBAR_H,
                });
            }
        }
        // Dropdown row hover (only while a topbar menu is open).
        if self.chrome_composited() && self.open_topbar_menu.is_some() {
            use crate::compositor::desktop_layer::{
                menu_item_x, DROPDOWN_W, TOPBAR_H, TOPBAR_MARGIN_X, TOPBAR_TOP,
            };
            let dx = TOPBAR_MARGIN_X + menu_item_x(self.dropdown_item());
            let dy = TOPBAR_TOP + TOPBAR_H + 4;
            let cx = self.state.cursor_x;
            let cy = self.state.cursor_y;
            let new_hover = if cx >= dx as i32
                && cy >= dy as i32
                && (cx as u32) < dx + DROPDOWN_W
                && (cy as u32) < dy + self.dropdown_h
            {
                self.active_menu().item_at((cy - dy as i32) as u32)
            } else {
                None
            };
            if new_hover != self.dropdown_hover {
                self.dropdown_hover = new_hover;
                self.dropdown_surface_dirty = true;
                self.queue_dirty_rect(DamageRect {
                    x: dx,
                    y: dy,
                    width: DROPDOWN_W.min(self.mode.width.saturating_sub(dx)),
                    height: self.dropdown_h,
                });
            }
        }
        let text_changed = old_state.text_input() != upstream.text_input();
        self.state.set_text_input(upstream.text_input());
        // Re-render the Search window's filtered list when the typed text changes.
        if self.search.visible && text_changed {
            super::desktop_layer::search_filter(self.state.text_input(), &mut self.search_filtered);
            // The row count changed → re-clamp the shared momentum extent (the
            // engine clamps position + target), then mirror the clamped PIXEL
            // offset back into the render state.
            self.search_set_extent();
            self.search.scroll = self.search_scroll.offset_px().max(0) as u32;
            self.search.surface_dirty = true;
            self.queue_dirty_rect(self.search_window_rect());
        }
        // Title-bar button hover `[– □ ×]` for the topmost window under the
        // cursor (TASK-0070 Phase 2; re-renders that window's title on change).
        self.update_title_hovers(self.state.cursor_x, self.state.cursor_y);
        // Pointer shape (TASK-0070 Phase 3): an active resize keeps its edge
        // shape; otherwise the topmost floating window's border band under the
        // cursor selects it; anything else restores the default pointer.
        self.update_cursor_shape_for_pointer(self.state.cursor_x, self.state.cursor_y);
        // (Search wheel handling moved into the unified stack-ordered wheel
        //  routing below — TASK-0070 Phase 1: topmost window under the cursor.)
        // C1: the proof panel is gone; `active_filter_idx` is now just a typed-text
        // change counter that still drives the filter selftest markers below.
        if !USE_DESKTOP_SHELL {
            self.active_filter_idx = filter_layout_variant_index(self.state.text_input());
        }
        self.refresh_observer_state();
        let button_hover_changed = old_button_hover != self.button_hover;
        if self.state == old_state && self.active_filter_idx == old_filter_idx {
            if button_hover_changed {
                self.note_button_hover_changed();
            }
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
        if button_hover_changed {
            self.note_button_hover_changed();
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
            // (The HOVER_LAYER spring is the glass-button highlight and is driven
            // by `note_button_hover_changed`, not by the hover test card.)
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

        // ── Wheel routing: the TOPMOST window under the cursor scrolls ──
        // Gate on the real signed delta (edge-accurate per update) rather than the
        // latched pulse booleans, so each notch is applied once with its magnitude.
        // The target comes from the SAME front-to-back stack order as presses
        // (window_scene SSOT) — occlusion applies to scrolling too: a window
        // covering another receives the wheel, and a dragged window keeps
        // scrolling wherever it currently sits.
        if upstream.wheel_delta_y != 0 {
            use crate::window_scene::WindowId;
            let (hit, hit_n) = self.windows.hit_order(USE_DESKTOP_SHELL);
            let target = hit[..hit_n].iter().copied().find(|&wid| match wid {
                WindowId::Chat => self.chat.contains(self.state.cursor_x, self.state.cursor_y),
                WindowId::Search => self.search.contains(self.state.cursor_x, self.state.cursor_y),
                WindowId::Settings => {
                    self.settings_win.contains(self.state.cursor_x, self.state.cursor_y)
                }
                WindowId::DslDemo => {
                    self.dsl_win.contains(self.state.cursor_x, self.state.cursor_y)
                }
                WindowId::AppClient => {
                    self.app_win.contains(self.state.cursor_x, self.state.cursor_y)
                }
                // The desktop base owns no window-chrome hit region (clicks reach
                // the shell as client input, not as a window drag/scroll target).
                WindowId::Desktop => false,
            });
            // Scroll diagnostic (rate-limited ~200ms): logs on every wheel input —
            // even when nothing moves — the routing target + full scroll state, so a
            // "scrolled but nothing happened" freeze is explained by VALUES, not guesses.
            let now = nsec().unwrap_or(0);
            if now.saturating_sub(self.chat_scroll_diag_ns) >= 200_000_000 {
                self.chat_scroll_diag_ns = now;
                let _ = debug_println(&alloc::format!(
                    "scroll-diag: in={} tgt={} cur=({},{}) chat_vis={} y={} pos={} target={} max={} base={} gl={}",
                    upstream.wheel_delta_y,
                    match target {
                        Some(WindowId::Chat) => "chat",
                        Some(WindowId::Search) => "search",
                        Some(WindowId::Settings) => "settings",
                        Some(WindowId::DslDemo) => "dsl",
                        Some(WindowId::AppClient) => "app",
                        Some(WindowId::Desktop) => "desktop",
                        None => "none",
                    },
                    self.state.cursor_x,
                    self.state.cursor_y,
                    self.chat.visible,
                    self.chat_scroll_y,
                    self.chat_list.scroll_offset().as_i32(),
                    self.chat_list.scroll_target(),
                    self.chat_list.max_scroll(),
                    self.chat_render_base,
                    self.gl_cursor_active,
                ));
            }
            match target {
                // The DSL demo window has no scrollable body (v0.1);
                // the app-client window routes input in R3 (not scroll-R1).
                Some(WindowId::DslDemo) | Some(WindowId::AppClient) | Some(WindowId::Desktop) => {}
                // Coalesce: accumulate this event's notches; `commit_scroll_input`
                // applies the frame's total ONCE (reactive, no per-event replay).
                Some(WindowId::Chat) => {
                    self.pending_chat_wheel =
                        self.pending_chat_wheel.saturating_add(upstream.wheel_delta_y);
                }
                // Search scrolls via the SHARED momentum engine, mapped EXACTLY
                // like chat: real notch count (magnitude preserved), wheel-up
                // (positive) moves content up (negative offset), ~3 rows/notch,
                // clamped per frame. (The old mapping dropped the magnitude AND
                // inverted the direction.)
                Some(WindowId::Search) => {
                    use super::desktop_layer::SEARCH_LIST_ROW_H;
                    const MAX_NOTCHES_PER_FRAME: i32 = 24;
                    let notches = upstream
                        .wheel_delta_y
                        .clamp(-MAX_NOTCHES_PER_FRAME, MAX_NOTCHES_PER_FRAME);
                    let step = 3 * SEARCH_LIST_ROW_H as i32;
                    self.search_scroll.scroll_wheel((-notches * step) as f32);
                    self.commit_search_scroll_position();
                }
                // The Settings panel is static — a wheel over it is a no-op
                // (consumed by the window, not a "miss").
                Some(WindowId::Settings) => {}
                // A wheel over no window is an HONEST no-op — but a rate-limited
                // diag line, never silence (the old silent fallthrough hid every
                // "scrolled and nothing happened" report).
                None => {
                    if now.saturating_sub(self.wheel_miss_diag_ns) >= 500_000_000 {
                        self.wheel_miss_diag_ns = now;
                        let _ = debug_println(&alloc::format!(
                            "windowd: wheel miss (x={} y={})",
                            self.state.cursor_x,
                            self.state.cursor_y,
                        ));
                    }
                }
            }
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

    /// Glass-button hover highlight: spring the button alpha (HOVER_LAYER drives
    /// `hover_opacity` in the GPU CB) and present the button rect. Independent of
    /// the proof-panel hover test card.
    pub(super) fn note_button_hover_changed(&mut self) {
        if !self.animation_driver.reduced_motion() {
            let spring = animation::SpringConfig {
                stiffness: 200.0,
                damping: 20.0,
                mass: 1.0,
                initial_velocity: 0.0,
            };
            let from = self.animated_scene.hover_opacity;
            let to = if self.button_hover { 1.0 } else { 0.0 };
            self.animation_driver.spring_to(HOVER_LAYER_ID, AnimProp::Opacity, from, to, spring);
        }
        let b = crate::interaction::button_rect(self.mode.width);
        self.queue_gpu_blit_rect(DamageRect { x: b.x, y: b.y, width: b.width, height: b.height });
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
