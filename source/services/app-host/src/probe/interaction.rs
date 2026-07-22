// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` interaction subsystem (pure move out of `main.rs`): body
//! taps + hover hit-testing, the WM resize re-layout, and the material glass
//! layer submission. No behavior change.

use super::*;

impl super::DslApp {
    /// Runs the interpreter's hit-testing for a body tap; on visible
    /// damage re-lays-out + refreshes the text runs. Returns whether a
    /// re-render is needed.
    pub(super) fn tap(&mut self, x: i32, y: i32) -> bool {
        use nexus_dsl_runtime::Damage;
        let tokens = tokens_for(self.theme_mode);
        let device = device_for(self.shell_profile, self.w);
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
        if !matches!(damage, Some(Damage::Paint) | Some(Damage::Layout)) {
            return false;
        }
        // Pretext discipline: ONLY layout-class damage re-runs the engine
        // (widget props — including text content — record Layout deps).
        // A paint-only change re-renders from the RETAINED boxes: the
        // pre-measured text + kept layout make that the cheap path.
        if matches!(damage, Some(Damage::Layout)) {
            self.relayout_retained();
        }
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
        true
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
            // consumers, never hover targets — no wash, no grow.
            .filter(|&id| self.interaction_sized(id));
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
        let device = device_for(self.shell_profile, new_w);
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
        // Scroll extents changed with the geometry: re-arm + re-clamp
        // after the fresh layout below (relayout path does the same).
        self.end_fired = false;
        let engine = nexus_layout::LayoutEngine::new();
        if let Ok(layout) = engine.layout_with_viewport(
            self.view.scene(),
            nexus_layout_types::FxPx::new(w as i32),
            Some(nexus_layout_types::FxPx::new(h as i32)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        ) {
            self.layout = layout;
            self.texts.clear();
            collect_texts(self.view.scene(), &mut 0, &mut self.texts);
        }
    }

    /// RFC-0075 tap-to-focus: resolve widget text focus at the tap point
    /// (AFTER `tap` — its dispatch may have re-laid-out). Returns the
    /// announcement for windowd when the focus state CHANGED:
    /// `(focused, field_kind, caret rect)`; `None` = unchanged.
    pub(super) fn text_focus_update(
        &mut self,
        x: i32,
        y: i32,
    ) -> Option<(bool, u8, (u16, u16, u16, u16))> {
        use nexus_display_proto::surface_text;
        let before = self.view.text_focus();
        let scroll = self.scroll_param();
        let snap = self.view.focus_text_at(
            &self.layout.boxes,
            nexus_layout_types::FxPx::new(x),
            nexus_layout_types::FxPx::new(y),
            scroll,
        );
        if snap == before {
            return None;
        }
        match snap {
            Some(s) => {
                let clamp = |v: i32| v.max(0).min(i32::from(u16::MAX)) as u16;
                let rect = self
                    .layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == s.box_id)
                    .map(|b| {
                        (
                            clamp(b.rect.x.0),
                            clamp(b.rect.y.0),
                            clamp(b.rect.width.0),
                            clamp(b.rect.height.0),
                        )
                    })
                    .unwrap_or((0, 0, 0, 0));
                let kind = if s.secure {
                    surface_text::SURFACE_FIELD_PASSWORD
                } else {
                    surface_text::SURFACE_FIELD_TEXT
                };
                Some((true, kind, rect))
            }
            None => Some((false, surface_text::SURFACE_FIELD_TEXT, (0, 0, 0, 0))),
        }
    }

    /// RFC-0075 committed-text delivery: insert into the FOCUSED field.
    /// Returns whether a re-render is needed.
    pub(super) fn text_commit(&mut self, text: &str) -> bool {
        use nexus_dsl_runtime::Damage;
        let tokens = tokens_for(self.theme_mode);
        let device = device_for(self.shell_profile, self.w);
        let locale = super::app_locale!(self);
        let damage = match self.view.insert_text(tokens, &device, &locale, text) {
            Ok(d) => d,
            Err(e) => {
                raw_marker(&alloc::format!("apphost: text commit ERR {e:?}"));
                None
            }
        };
        if !matches!(damage, Some(Damage::Paint) | Some(Damage::Layout)) {
            return false;
        }
        // One-shot end-to-end proof (RFC-0075): the first commit that changed
        // the focused field. Count-only — typed text NEVER hits markers.
        static COMMIT_MARKED: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);
        if !COMMIT_MARKED.swap(true, core::sync::atomic::Ordering::Relaxed) {
            raw_marker("apphost: text commit applied");
        }
        if matches!(damage, Some(Damage::Layout)) {
            self.relayout_retained();
        }
        true
    }

    /// RFC-0075 editing-action delivery (imed wire `ACTION_*` in `aux`).
    /// Backspace edits the focused field; Escape drops widget focus (the
    /// caller announces the transition). Enter/Tab are page-level concerns
    /// (deferred — no focus traversal yet). Returns re-render need.
    pub(super) fn text_action(&mut self, action: u8) -> bool {
        use nexus_dsl_runtime::Damage;
        match action {
            nexus_wire::imed::ACTION_BACKSPACE => {
                let tokens = tokens_for(self.theme_mode);
                let device = device_for(self.shell_profile, self.w);
                let locale = super::app_locale!(self);
                let damage = match self.view.backspace_text(tokens, &device, &locale) {
                    Ok(d) => d,
                    Err(e) => {
                        raw_marker(&alloc::format!("apphost: text action ERR {e:?}"));
                        None
                    }
                };
                if matches!(damage, Some(Damage::Layout)) {
                    self.relayout_retained();
                }
                matches!(damage, Some(Damage::Paint) | Some(Damage::Layout))
            }
            _ => false,
        }
    }

    /// RFC-0075 tap-to-focus announcement: resolve widget focus at the tap
    /// point and, on a TRANSITION, send `OP_SURFACE_TEXT_FOCUS` to windowd
    /// (which relays to imed). Marker carries no text content.
    pub(super) fn announce_text_focus(
        &mut self,
        client: &KernelClient,
        surface_id: u32,
        x: i32,
        y: i32,
    ) {
        use nexus_display_proto::surface_text;
        if surface_id == 0 {
            return; // no surface yet — nothing to claim
        }
        let Some((focused, field_kind, caret)) = self.text_focus_update(x, y) else {
            return;
        };
        let f = surface_text::encode_surface_text_focus(surface_id, focused, field_kind, caret);
        let _ = client.send(&f, Wait::NonBlocking);
        raw_marker(if focused { "apphost: text focus set" } else { "apphost: text focus cleared" });
    }

    /// RFC-0075 composed-text delivery: decode an `OP_SURFACE_TEXT` frame and
    /// apply it to the focused field. Returns re-render need (`false` also
    /// for non-text frames). Text content NEVER hits markers/logs.
    pub(super) fn apply_surface_text(&mut self, frame: &[u8]) -> bool {
        use nexus_display_proto::surface_text as st;
        let Some((tkind, aux, text)) = st::decode_surface_text(frame) else {
            return false;
        };
        match tkind {
            st::SURFACE_TEXT_COMMIT => self.text_commit(text),
            st::SURFACE_TEXT_ACTION => self.text_action(aux),
            // Preedit display lands with the candidate UI (RFC-0075 Phase 3).
            _ => false,
        }
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
            let _ = client.send(&req, Wait::NonBlocking);
        }
        (dirty, rows)
    }

    /// R1 layer seam: submit the material-tagged glass regions of the current
    /// layout to windowd (`OP_SURFACE_LAYERS`). Each `LayoutBox` whose
    /// `.material()` is glass becomes a `LayerDesc` (surface-local rect +
    /// level + radius + shadow); windowd composites each as a real frosted
    /// `nexus-gfx` layer over the wallpaper. Re-sent whenever the layout
    /// changes (mount + re-layout). No glass nodes ⇒ empty list ⇒ windowd
    /// composites the surface with the default treatment (unchanged).
    pub(super) fn submit_layers(&self, client: &KernelClient, surface_id: u32) {
        use nexus_layout_types::{GlassLevel, SurfaceMaterial};
        let clamp = |v: i32| v.max(0).min(u16::MAX as i32) as u16;
        let mut layers = [wire::LayerDesc::default(); wire::MAX_SURFACE_LAYERS];
        let mut n = 0;
        for b in &self.layout.boxes {
            if n >= wire::MAX_SURFACE_LAYERS {
                break;
            }
            let glass_level = match b.visual.material {
                SurfaceMaterial::Glass(GlassLevel::Panel) => wire::GLASS_PANEL,
                SurfaceMaterial::Glass(GlassLevel::Card) => wire::GLASS_CARD,
                SurfaceMaterial::Glass(GlassLevel::Subtle) => wire::GLASS_SUBTLE,
                SurfaceMaterial::Glass(GlassLevel::Window) => wire::GLASS_WINDOW,
                SurfaceMaterial::Glass(GlassLevel::Overlay) => wire::GLASS_OVERLAY,
                SurfaceMaterial::Opaque => continue,
            };
            layers[n] = wire::LayerDesc {
                x: clamp(b.rect.x.0),
                y: clamp(b.rect.y.0),
                w: clamp(b.rect.width.0),
                h: clamp(b.rect.height.0),
                material: wire::MATERIAL_GLASS,
                glass_level,
                radius: b.visual.corner_radius.top_left.0.clamp(0, 255) as u8,
                shadow_alpha: if b.visual.shadow.is_some() { 80 } else { 0 },
            };
            n += 1;
        }
        let mut buf = [0u8; wire::SURFACE_LAYERS_MAX_LEN];
        let len = wire::encode_surface_layers(surface_id, &layers[..n], &mut buf);
        let _ = client.send(&buf[..len], Wait::NonBlocking);
        raw_marker(&alloc::format!("apphost: submitted {n} layers"));
    }
}
