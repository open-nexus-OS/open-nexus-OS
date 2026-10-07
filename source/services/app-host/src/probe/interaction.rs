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
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let scroll = self.scroll_param();
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
            scroll,
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
            scroll,
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
                // live harness has.
                self.trace_tap(x, y, hit, damage);
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
            // Take (not clone) the previous retained state: `relayout_retained`
            // rebuilds both, and the old lists feed the damage diff — a
            // structural tap whose change is bounded (an overlay toggles, a
            // row swaps its value) then re-renders ONLY the changed rows
            // instead of the whole frame. On the QEMU TCG boot a full-frame
            // CPU raster costs >1s per click, which read as "menus never
            // open" (the user's next click toggled the menu back closed
            // before the first one ever presented).
            let old_boxes = core::mem::take(&mut self.layout.boxes);
            let old_texts = core::mem::take(&mut self.texts);
            self.relayout_retained();
            self.tap_render_span = crate::layout_diff::changed_row_span(
                &old_boxes,
                &self.layout.boxes,
                &old_texts,
                &self.texts,
                self.h as i32,
            );
            self.layers_dirty = true;
        }
        self.trace_tap(x, y, hit, damage);
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

    /// Whether the glass-region set may have changed since the last
    /// `submit_layers` (consumed once).
    pub(super) fn take_layers_dirty(&mut self) -> bool {
        core::mem::take(&mut self.layers_dirty)
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

    /// Pointer motion (`INPUT_KIND_MOVE`): re-resolve the hovered
    /// interactive box (same hit-test the Tap routing uses). Returns the
    /// union ROW SPAN of the old+new hovered boxes when the target
    /// changed (`None` = no change) — a PAINT-only change: the caller
    /// re-renders exactly that span; layout and boxes stay retained.
    pub(super) fn hover(&mut self, x: i32, y: i32) -> Option<(i32, i32)> {
        let scroll = self.scroll_param();
        let target = self
            .view
            .hover_box_id_scrolled(
                &self.layout.boxes,
                "Tap",
                nexus_layout_types::FxPx::new(x),
                nexus_layout_types::FxPx::new(y),
                scroll,
            )
            // Container catch-alls (overlay backdrop, panel body) are TAP
            // consumers, never hover targets. The gate here is the WASH's
            // (short = a control), not the GROW's — `interaction_hover` applies
            // the stricter `interaction_sized` itself, so a full-width list row
            // washes without growing. Filtering here with the grow's rule is
            // what left rows and sidebar entries with no hover at all.
            .filter(|&id| crate::hover_wash::hover_washable(&self.layout.boxes, id));
        if target == self.hovered {
            return None;
        }
        let old = core::mem::replace(&mut self.hovered, target);
        // Interaction motion (handoff): grow the newly hovered control with a
        // soft spring, shrink the old one back — the caller arms the frame
        // pulse (`anim_active`) so the springs tick on the real cadence.
        self.interaction_hover(old, target);
        self.hover_span(old, target)
    }

    /// Whether the pointer sits over an editable (Change-bound) field —
    /// `Some(state)` only on a CHANGE; the caller sends the windowd cursor
    /// hint then (I-beam over fields, RFC-0075). Same hit-test family as
    /// tap-to-focus, so hint and focusability can never disagree.
    pub(super) fn text_hover(&mut self, x: i32, y: i32) -> Option<bool> {
        let scroll = self.scroll_param();
        let over = self
            .view
            .hover_box_id_scrolled(
                &self.layout.boxes,
                "Change",
                nexus_layout_types::FxPx::new(x),
                nexus_layout_types::FxPx::new(y),
                scroll,
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

    /// Pointer left the surface (`INPUT_KIND_LEAVE`): clear the wash.
    /// Returns the cleared box's row span for the partial repaint.
    pub(super) fn hover_clear(&mut self) -> Option<(i32, i32)> {
        let old = self.hovered.take();
        self.interaction_hover(old, None);
        self.hover_span(old, None)
    }

    /// Union row span (y0, y1 exclusive; surface-clamped) of two hover
    /// anchors' boxes — the exact rows a hover change repaints.
    pub(super) fn hover_span(&self, a: Option<usize>, b: Option<usize>) -> Option<(i32, i32)> {
        let mut span: Option<(i32, i32)> = None;
        for id in [a, b].into_iter().flatten() {
            if let Some(bx) = self.layout.boxes.iter().find(|bb| bb.node_id == id) {
                let y0 = bx.rect.y.0.max(0);
                let y1 = (bx.rect.y.0 + bx.rect.height.0).min(self.h as i32);
                if y0 < y1 {
                    span = Some(match span {
                        Some((s0, s1)) => (s0.min(y0), s1.max(y1)),
                        None => (y0, y1),
                    });
                }
            }
        }
        span
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

    /// Wheel impulse (`INPUT_KIND_WHEEL`, moved out of the main event loop —
    /// structure-gate): scroll physics + EndReached + frame-pulse arming.
    /// Banded surfaces are compositor-scrolled (windowd shifts the gpud layer
    /// `src_row` and pushes `INPUT_KIND_SCROLL_POS`), so this is defensive
    /// there. Returns `(dirty, row_span)` — span `None` with dirty = full.
    pub(super) fn wheel_event(
        &mut self,
        client: &KernelClient,
        surface_id: u32,
        y_raw: u16,
        rx_markers: &mut u32,
    ) -> (bool, Option<(i32, i32)>) {
        if *rx_markers < 40 {
            *rx_markers += 1;
            let d = wire::wheel_delta_from_wire(y_raw);
            raw_marker(&alloc::format!("APPHOST: wheel rx n={rx_markers} d={d}"));
        }
        if self.banded {
            return (false, None);
        }
        let delta = wire::wheel_delta_from_wire(y_raw);
        // Paged viewport (`.scroll(paged)`): a notch turns the PAGE — glide
        // snap + the container's PageNext/PagePrev trigger (store page sync).
        if matches!(
            self.scroll_region_axis(),
            Some((_, _, _, nexus_layout_types::ScrollAxis::Paged))
        ) {
            let (span, dir) = self.pager_wheel(delta);
            let mut dirty = span.is_some();
            let mut rows = span;
            if dir != 0 && self.fire_pager_trigger(dir > 0) {
                dirty = true;
                rows = None; // model changed (page index): full repaint
            }
            if self.momentum_active() {
                let req = wire::encode_surface_frame_req(surface_id);
                let _ = client.send(&req, Wait::Blocking);
            }
            return (dirty, rows);
        }
        let (span, end) = self.scroll_wheel(delta);
        let mut dirty = false;
        let mut rows = span;
        if span.is_some() {
            dirty = true;
        }
        if end && self.fire_end_reached() {
            dirty = true;
            rows = None; // model changed: full repaint
        }
        // Choreographer contract: while the ease/fling is live, ask the
        // compositor for ONE frame pulse (physics ticks on the real cadence).
        if self.momentum_active() {
            let req = wire::encode_surface_frame_req(surface_id);
            let _ = client.send(&req, Wait::Blocking);
        }
        (dirty, rows)
    }
}
