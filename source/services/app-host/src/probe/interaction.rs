// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` interaction subsystem (pure move out of `main.rs`): body
//! taps + hover hit-testing, the WM resize re-layout, and the material glass
//! layer submission. No behavior change.

use super::*;

/// RFC-0075 text delivery (focus, commit, actions, IME strip) — its own file.
mod text_edit;
mod text_io;

/// What a tap actually did — the three cases the single `bool` used to
/// flatten into one, which made an absorbed tap indistinguishable from a
/// coordinate-mapping bug in the boot log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TapOutcome {
    /// A handler ran and the scene needs repainting.
    Repainted,
    /// A handler ran; nothing on THIS surface changed (a `PanelNoop`-style
    /// absorber, or a control whose effect lands in another service).
    HandledQuietly,
    /// The point resolved to no handler at all — the only case worth a
    /// `tap miss` marker and a handler-box dump.
    NoHandler,
}

impl super::DslApp {
    /// Runs the interpreter's hit-testing for a body tap; on visible
    /// damage re-lays-out + refreshes the text runs. Returns whether a
    /// re-render is needed.
    pub(super) fn tap(&mut self, x: i32, y: i32) -> TapOutcome {
        use nexus_dsl_runtime::Damage;
        // The press a drag may grow from (`probe/drag.rs`; windowd's DRAG frames follow).
        self.arm_drag(x, y);
        self.pointer = Some((x, y));
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        // The page's scroll layers at their offsets: the hit-test sees what the compositor shows.
        let (views, nv) = self.hit_views();
        // Interaction motion (handoff "Press: instant down, springy release"):
        // the pressed control dips to 92% and pops back elastically. Resolved
        // with the SAME hit-test the dispatch below uses, BEFORE the re-emit
        // (node ids are stable across a paint-damage dispatch; the retain in
        // `anim_sync` carries the bounce across a re-emit).
        let hit = self.view.hover_box_id_scrolled(
            &self.layout.boxes,
            "Tap",
            nexus_layout_types::FxPx::new(x),
            nexus_layout_types::FxPx::new(y),
            &views[..nv],
        );
        // Some kinds animate a PART instead of the whole control (the toggle's
        // thumb): the handler carries a structural box-id offset (registry
        // `press_offset`, resolved at emit time — no widget kind leaks here).
        let press_part = hit.map(|h| {
            let off = self
                .view
                .handlers()
                .iter()
                .find(|(box_id, _)| *box_id == h)
                .map_or(0, |(_, e)| e.press_offset as usize);
            h + off
        });
        match (hit, press_part) {
            (Some(h), Some(p)) if p == h => self.interaction_press(h),
            _ => {} // part-press (toggle thumb) fires AFTER the flip below
        }
        // The part's pre-dispatch x (the thumb slides on a toggle flip; the
        // slide-in animates from old − new).
        let part_x_before = press_part
            .and_then(|p| self.layout.boxes.iter().find(|b| b.node_id == p).map(|b| b.rect.x.0));
        let locale = super::app_locale!(self);
        let damage = match self.view.pointer_scrolled(
            tokens,
            &device,
            &locale,
            &mut self.host,
            &self.layout.boxes,
            "Tap",
            nexus_layout_types::FxPx::new(x),
            nexus_layout_types::FxPx::new(y),
            &views[..nv],
        ) {
            Ok(d) => d,
            Err(e) => {
                // A dispatch error must never be silent: the store may have
                // committed while the re-emit failed (stale UI). Bounded by
                // user tap rate.
                raw_marker(&alloc::format!("apphost: tap dispatch ERR {e:?}"));
                None
            }
        };
        // A tap on the open modal's backdrop fired its `on Dismiss` (TASK-0074
        // D3): name the reason for the ladder.
        self.note_dismiss();
        // Drain the in-process pager verb a dispatched effect may have parked
        // (`svc.shell.scrollToPage`, the dot tap) — the glide starts now and
        // the caller's `momentum_active()` frame-pulse arming keeps it ticking.
        if let Some(page) = self.host.pending_scroll_page.take() {
            self.pager_scroll_to(page);
        }
        if !matches!(damage, Some(Damage::Paint) | Some(Damage::Layout)) {
            // A handler DID run (an absorber like the panel's `PanelNoop`, or
            // one whose effect is off-surface such as a shell-profile switch) —
            // it just produced no repaint. That is not a miss, and reporting it
            // as one sends the next reader hunting a hit-test bug that is not
            // there.
            if hit.is_some() {
                // Traced like a repainting tap (bounded): a quiet tap is still
                // a tap that landed somewhere — the one position oracle the
                // live harness has. Nothing will present: settled at once.
                if self.trace_tap(x, y, hit, damage) {
                    raw_marker("apphost: tap settled");
                }
                return TapOutcome::HandledQuietly;
            }
            return TapOutcome::NoHandler;
        }
        // Pretext discipline: ONLY layout-class damage re-runs the engine
        // (widget props — including text content — record Layout deps).
        // A paint-only change re-renders from the RETAINED boxes: the
        // pre-measured text + kept layout make that the cheap path.
        self.tap_render_span = None;
        if matches!(damage, Some(Damage::Layout)) {
            self.relayout_bounded();
        }
        self.tap_present_owed = self.trace_tap(x, y, hit, damage);
        // Part-press (toggle thumb): the flip has re-laid-out, the knob sits
        // at its new end — stretch it along the travel axis and slide it in
        // from where it was (node ids are stable across the re-emit).
        if let (Some(h), Some(p), Some(x0)) = (hit, press_part, part_x_before) {
            if p != h {
                let x1 =
                    self.layout.boxes.iter().find(|b| b.node_id == p).map_or(x0, |b| b.rect.x.0);
                self.interaction_toggle_thumb(p, (x0 - x1) as f32);
            }
        }
        // The dispatch re-emitted the scene: reconcile the animation driver
        // with the new intents (a changed `.animate`/`.effect` value starts
        // its motion). The caller arms the frame pulse when `anim_active`.
        self.anim_sync();
        TapOutcome::Repainted
    }

    /// Re-lays out after a structural change and keeps the rows it changed as the next present's
    /// span (`merge_tap_damage`): a bounded change — an overlay toggles, a row swaps its value, a
    /// typed character — re-renders ONLY those rows instead of the whole frame (on the board a
    /// full-frame CPU raster costs most of a second; on the QEMU TCG boot it read as "menus never
    /// open"). The previous retained state is taken, not cloned: the relayout rebuilds both
    /// lists and the old ones feed the diff.
    pub(super) fn relayout_bounded(&mut self) {
        let old_boxes = core::mem::take(&mut self.layout.boxes);
        let old_texts = core::mem::take(&mut self.texts);
        if !self.relayout_retained() {
            // The layout failed and was abandoned: the screen still shows the retained boxes —
            // they go back, and nothing is repainted (an empty span).
            self.layout.boxes = old_boxes;
            self.texts = old_texts;
            self.tap_render_span = Some((0, 0));
            return;
        }
        self.tap_render_span = crate::layout_diff::changed_row_span(
            &old_boxes,
            &self.layout.boxes,
            &old_texts,
            &self.texts,
            self.layout_bottom(),
        );
        self.tree_dirty = true;
    }

    /// Folds the last tap's damage span into the present-loop's dirty state:
    /// `None` = genuine full repaint; an EMPTY span = pixels identical, no
    /// repaint needed; a real span unions with whatever is already pending
    /// (an earlier full request always wins).
    pub(super) fn merge_tap_damage(
        &mut self,
        dirty: &mut bool,
        dirty_rows: &mut Option<(i32, i32)>,
    ) {
        match self.tap_render_span.take() {
            None => {
                *dirty = true;
                *dirty_rows = None;
            }
            Some((y0, y1)) if y1 <= y0 => {}
            Some(span) => {
                *dirty_rows = match (*dirty, *dirty_rows) {
                    (true, None) => None,
                    (_, Some((a0, a1))) => Some((a0.min(span.0), a1.max(span.1))),
                    (false, None) => Some(span),
                };
                *dirty = true;
            }
        }
    }

    /// Reserve `top` surface rows for the shell status bar and re-lay-out.
    ///
    /// The compositor owns the number (`OP_SURFACE_RECT`'s long-reserved `y`):
    /// it alone knows the bar height AND whether this window is maximized. A
    /// chromeless fullscreen window keeps its full-height surface — the glass
    /// still reaches y=0 and the translucent bar sits on it — while its
    /// content starts below. Floating windows get 0 and are placed below the
    /// bar instead.
    pub(super) fn apply_safe_area_top(&mut self, top: u32) {
        let px = nexus_layout_types::FxPx::new(top as i32);
        if !self.view.set_safe_area_top(px) {
            // A leaf page root has no content box to pad. Say so instead of
            // silently rendering under the bar.
            raw_marker("apphost: FAIL safe-area (page root is not a container)");
            return;
        }
        self.relayout_retained();
    }

    /// Pointer motion (`INPUT_KIND_MOVE`): re-resolve the hovered interactive box (same
    /// hit-test the Tap routing uses, over the scroll layers at their offsets). A change is
    /// MOTION — the wash layer moves (RFC-0109) — never a raster. Returns whether the hovered
    /// control changed.
    pub(super) fn hover(&mut self, x: i32, y: i32) -> bool {
        self.pointer = Some((x, y));
        let (views, nv) = self.hit_views();
        let target = self
            .view
            .hover_box_id_scrolled(
                &self.layout.boxes,
                "Tap",
                nexus_layout_types::FxPx::new(x),
                nexus_layout_types::FxPx::new(y),
                &views[..nv],
            )
            // Container catch-alls (overlay backdrop, panel body) are TAP consumers, never hover
            // targets. The gate here is the WASH's (short = a control), not the GROW's —
            // `interaction_hover` applies the stricter `interaction_sized` itself, so a
            // full-width list row washes without growing.
            .filter(|&id| crate::hover_wash::hover_washable(&self.layout.boxes, id));
        if target == self.hovered {
            return false;
        }
        let old = core::mem::replace(&mut self.hovered, target);
        self.motion_dirty = true;
        // Interaction motion (handoff): grow the newly hovered control with a soft spring,
        // shrink the old one back — the caller arms the frame pulse (`anim_active`).
        self.interaction_hover(old, target);
        true
    }

    /// The content moved under a still pointer (a scroll step): what it hovers is resolved
    /// again — the wash follows the control now under the pointer.
    pub(super) fn rehover(&mut self) {
        if let Some((x, y)) = self.pointer {
            let _ = self.hover(x, y);
        }
    }

    /// Whether the pointer sits over an editable (Change-bound) field —
    /// `Some(state)` only on a CHANGE; the caller sends the windowd cursor
    /// hint then (I-beam over fields, RFC-0075). Same hit-test family as
    /// tap-to-focus, so hint and focusability can never disagree.
    pub(super) fn text_hover(&mut self, x: i32, y: i32) -> Option<bool> {
        let (views, nv) = self.hit_views();
        let over = self
            .view
            .hover_box_id_scrolled(
                &self.layout.boxes,
                "Change",
                nexus_layout_types::FxPx::new(x),
                nexus_layout_types::FxPx::new(y),
                &views[..nv],
            )
            .is_some();
        if over == self.hover_text {
            return None;
        }
        self.hover_text = over;
        Some(over)
    }

    /// Pointer left the surface: drop the text-hover latch. Returns whether
    /// it was set (the caller clears the windowd hint then).
    pub(super) fn text_hover_clear(&mut self) -> bool {
        core::mem::replace(&mut self.hover_text, false)
    }

    /// Pointer left the surface (`INPUT_KIND_LEAVE`): the wash goes (motion). Returns whether a
    /// control was hovered.
    pub(super) fn hover_clear(&mut self) -> bool {
        self.pointer = None;
        let old = self.hovered.take();
        self.interaction_hover(old, None);
        if old.is_some() {
            self.motion_dirty = true;
        }
        old.is_some()
    }

    /// Re-emits the scene under a NEW width class (mobile-first breakpoints:
    /// the resize crossed a `device.sizeClass` boundary, so `if device.*`
    /// arms select a different structure). Store state survives — this is a
    /// re-emit, never a remount. The caller runs `resize` (relayout) after.
    pub(super) fn reemit_for_size_class(&mut self, new_w: u32) {
        let tokens = tokens_for(self.theme_mode);
        self.w = new_w;
        let device =
            device_for(self.shell_profile, new_w, &self.locale_tag, &self.keymap, self.theme_mode);
        let locale = super::app_locale!(self);
        if self.view.reemit(tokens, &device, &locale).is_err() {
            raw_marker("apphost: FAIL size-class reemit");
            return;
        }
        raw_marker("apphost: size-class reemit");
        // New structure ⇒ new node ids: reconcile animations + drop the
        // stale hover anchor (the next MOVE re-resolves).
        self.hovered = None;
        self.anim_sync();
    }

    /// WM resize (`OP_SURFACE_RECT`): re-lay-out the current view at the new
    /// surface size — WITHOUT resetting store state (a remount would). Both
    /// width AND height take effect (the scene reflows to `w`; the render
    /// bound uses `h`). The caller re-renders into the freshly-sized VMO.
    /// Publish this app's windowd surface id to the effect host — it rides
    /// in `CONTROL_WIN_*` values (window-kit app-menu window actions).
    pub(super) fn set_surface_id(&mut self, id: u32) {
        // A new surface: windowd holds no structure of it until the next is sent.
        self.sent_tree = nexus_display_proto::surface_layers::SurfaceTree::EMPTY;
        #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
        {
            self.host.surface_id = id;
        }
        #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
        let _ = id;
    }

    pub(super) fn resize(&mut self, w: u32, h: u32) {
        self.w = w;
        self.h = h;
        self.row_scratch.resize(w as usize * 4, 0);
        // Box geometry moves under the pointer; the next MOVE re-resolves.
        self.hovered = None;
        // ONE layout phase (TASK-0077C P2b): this used to repeat the layout
        // call and the text collection by hand — "relayout path does the
        // same", its own comment said — which left the resize frame outside
        // the generation arena and kept `texts` across frames with `clear()`.
        // The shared phase re-arms and re-clamps scroll extents as well.
        self.relayout_retained();
    }
}
