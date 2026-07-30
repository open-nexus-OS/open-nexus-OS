// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` material-glass layer declaration (`OP_SURFACE_LAYERS`):
//! split out of `interaction.rs` (module-size ratchet) — the hit-testing
//! stays there, the compositor-region seam lives here.

use super::*;

impl super::DslApp {
    /// R1 layer seam: submit the material-tagged glass regions of the current
    /// layout to windowd (`OP_SURFACE_LAYERS`). Each `LayoutBox` whose
    /// `.material()` is glass becomes a `LayerDesc` (surface-local rect +
    /// level + radius + shadow); windowd composites each as a real frosted
    /// `nexus-gfx` layer over the wallpaper. No glass nodes ⇒ empty list ⇒
    /// windowd composites the surface with the default treatment (unchanged).
    ///
    /// WHEN it is re-sent is the subtle part. The rects are SURFACE-LOCAL, so
    /// they are stale the moment the surface is re-created at a new size — and
    /// the only caller besides mount sits behind `span.is_none()`, i.e. it runs
    /// on a FULL present only. A resize that left a partial damage span behind
    /// therefore never re-declared anything, and windowd kept compositing the
    /// pre-resize rects: a maximized file manager painted its 960-wide panes in
    /// the corner of a 1280-wide window. The resize path now forces a full
    /// present, and windowd clears the list on create as well
    /// (`app_surface::clear_app_layers`) so neither side can carry a stale set
    /// alone.
    pub(super) fn submit_layers(&self, client: &KernelClient, surface_id: u32) {
        use nexus_layout_types::{GlassLevel, SurfaceMaterial};
        let clamp = |v: i32| v.max(0).min(u16::MAX as i32) as u16;
        let mut layers = [wire::LayerDesc::default(); wire::MAX_SURFACE_LAYERS];
        let mut n = 0;
        let mut dropped = 0usize;
        for b in &self.layout.boxes {
            // Only glass ROOTS become compositor regions: nested glass
            // blends into its parent's pixels (`glass_nested`), so a region
            // per inner card would both re-blur what the root already
            // frosts AND overflow the 16-layer wire cap — a settings-like
            // page carries ~27 glass nodes but only 3-4 roots.
            if b.glass_nested {
                continue;
            }
            if n >= wire::MAX_SURFACE_LAYERS {
                dropped += 1;
                continue;
            }
            // The wire `glass_level` is a BLUR BUCKET, not the material: the
            // tint, shine, hairline and gradient are already painted into this
            // surface's own pixels by `Style::glass`, and windowd reads the
            // level only to pick a backdrop radius (`scene.rs`: panel/overlay
            // 40, card 20, subtle 8). So the two window levels ride the bucket
            // whose radius the theme authors for them — `windowPane` 20 = card,
            // `windowBar` 40 = panel — instead of costing two new wire values
            // that would carry no extra information.
            let glass_level = match b.visual.material {
                SurfaceMaterial::Glass(GlassLevel::Panel) => wire::GLASS_PANEL,
                SurfaceMaterial::Glass(GlassLevel::Card) => wire::GLASS_CARD,
                SurfaceMaterial::Glass(GlassLevel::Subtle) => wire::GLASS_SUBTLE,
                SurfaceMaterial::Glass(GlassLevel::Window) => wire::GLASS_WINDOW,
                SurfaceMaterial::Glass(GlassLevel::WindowPane) => wire::GLASS_CARD,
                SurfaceMaterial::Glass(GlassLevel::WindowBar) => wire::GLASS_PANEL,
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
        if dropped > 0 {
            // A silent `break` here once cost half a page its glass — the
            // cap is a wire constant, exceeding it must be visible.
            raw_marker(&alloc::format!("apphost: {dropped} glass regions over the layer cap"));
        }
    }
}
