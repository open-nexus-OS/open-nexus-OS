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
        use nexus_dsl_runtime::{Damage, IdentityLocale};
        let tokens = tokens_for(self.theme_mode);
        let device = device_for(self.shell_profile);
        let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
        let scroll = self.scroll_param();
        let damage = self
            .view
            .pointer_scrolled(
                tokens,
                &device,
                &locale,
                &mut self.host,
                &self.layout.boxes,
                "Tap",
                nexus_layout_types::FxPx::new(x),
                nexus_layout_types::FxPx::new(y),
                scroll,
            )
            .ok()
            .flatten();
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
        true
    }

    /// Pointer motion (`INPUT_KIND_MOVE`): re-resolve the hovered
    /// interactive box (same hit-test the Tap routing uses). Returns the
    /// union ROW SPAN of the old+new hovered boxes when the target
    /// changed (`None` = no change) — a PAINT-only change: the caller
    /// re-renders exactly that span; layout and boxes stay retained.
    pub(super) fn hover(&mut self, x: i32, y: i32) -> Option<(i32, i32)> {
        let scroll = self.scroll_param();
        let target = self.view.hover_box_id_scrolled(
            &self.layout.boxes,
            "Tap",
            nexus_layout_types::FxPx::new(x),
            nexus_layout_types::FxPx::new(y),
            scroll,
        );
        if target == self.hovered {
            return None;
        }
        let old = core::mem::replace(&mut self.hovered, target);
        self.hover_span(old, target)
    }

    /// Pointer left the surface (`INPUT_KIND_LEAVE`): clear the wash.
    /// Returns the cleared box's row span for the partial repaint.
    pub(super) fn hover_clear(&mut self) -> Option<(i32, i32)> {
        let old = self.hovered.take();
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

    /// WM resize (`OP_SURFACE_RECT`): re-lay-out the current view at the new
    /// surface size — WITHOUT resetting store state (a remount would). Both
    /// width AND height take effect (the scene reflows to `w`; the render
    /// bound uses `h`). The caller re-renders into the freshly-sized VMO.
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
