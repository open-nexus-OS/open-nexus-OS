// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` material regions — the region list of the surface's structure
//! (`OP_SURFACE_TREE`, RFC-0109; the R1 seam): split out of `interaction.rs` (module-size
//! ratchet) — the hit-testing stays there, the compositor-region seam lives here.

use super::*;

impl super::DslApp {
    /// The material-tagged regions of the current layout (the R1 seam): each `LayoutBox` whose
    /// `.material()` is glass becomes a `LayerDesc` (base-layer rect + level + radius +
    /// shadow); windowd composites each as a real frosted `nexus-gfx` layer. They ride the
    /// surface's structure (`band::send_tree`), so a re-created surface declares them anew with
    /// its first present — and windowd clears them at create, so neither side can carry a stale
    /// set alone. A box of a band's content is no region: it moves with its layer, its tint is in
    /// the band. Returns the count written into `out`.
    pub(super) fn regions(&self, out: &mut [wire::LayerDesc; wire::MAX_SURFACE_LAYERS]) -> usize {
        use nexus_layout_types::{GlassLevel, SurfaceMaterial};
        // The region inside the base layer (windowd refuses a structure with one outside it).
        let (sw, sh) = (self.w as i32, self.h as i32);
        let fit = |b: &nexus_layout::LayoutBox| {
            let (x0, y0) = (b.rect.x.0.max(0), b.rect.y.0.max(0));
            let x1 = (b.rect.x.0 + b.rect.width.0).min(sw);
            let y1 = (b.rect.y.0 + b.rect.height.0).min(sh);
            (x1 > x0 && y1 > y0).then(|| (x0 as u16, y0 as u16, (x1 - x0) as u16, (y1 - y0) as u16))
        };
        let layers = out;
        let mut n = 0;
        let mut dropped = 0usize;
        for b in &self.layout.boxes {
            if self.in_band(b) {
                continue;
            }
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
            // The materials rules (docs/dev/ui/foundations/visual/materials.md, TASK-0337)
            // inside a WINDOW: the content pane is an OPAQUE region (no blur, an occluder —
            // its pixels are painted opaque); bars, cards and subtle rows are tint, painted
            // into the surface and no region at all; the window's own glass, panels and
            // overlays (the functional layer) stay glass. The desktop base keeps every
            // region as glass: its regions sit straight on the wallpaper.
            let full = b.visual.opacity.is_none_or(|o| o.0 >= 255);
            if wire::theme_is_solid(self.theme_mode) {
                // Solid materials (TASK-0337): every material box is painted at its tint's full
                // opacity — an opaque region (an occluder), never a blur.
                if matches!(b.visual.material, SurfaceMaterial::Opaque) || !full {
                    continue;
                }
                let Some((x, y, w, h)) = fit(b) else { continue };
                layers[n] = wire::LayerDesc {
                    x,
                    y,
                    w,
                    h,
                    material: wire::MATERIAL_OPAQUE,
                    glass_level: 0,
                    radius: b.visual.corner_radius.top_left.0.clamp(0, 255) as u8,
                    shadow_alpha: if b.visual.shadow.is_some() { 80 } else { 0 },
                };
                n += 1;
                continue;
            }
            let (material, glass_level) = match b.visual.material {
                SurfaceMaterial::Glass(GlassLevel::Panel) => {
                    (wire::MATERIAL_GLASS, wire::GLASS_PANEL)
                }
                SurfaceMaterial::Glass(GlassLevel::Card) if self.desktop => {
                    (wire::MATERIAL_GLASS, wire::GLASS_CARD)
                }
                SurfaceMaterial::Glass(GlassLevel::Subtle) if self.desktop => {
                    (wire::MATERIAL_GLASS, wire::GLASS_SUBTLE)
                }
                SurfaceMaterial::Glass(GlassLevel::Window) => {
                    (wire::MATERIAL_GLASS, wire::GLASS_WINDOW)
                }
                // Opaque only while it is drawn at full opacity: a faded pane shows what lies
                // below it, and an occluder that is not would leave a hole.
                SurfaceMaterial::Glass(GlassLevel::WindowPane) if full => {
                    (wire::MATERIAL_OPAQUE, 0)
                }
                SurfaceMaterial::Glass(GlassLevel::WindowPane) => continue,
                SurfaceMaterial::Glass(GlassLevel::Overlay) => {
                    (wire::MATERIAL_GLASS, wire::GLASS_OVERLAY)
                }
                SurfaceMaterial::Glass(GlassLevel::Card)
                | SurfaceMaterial::Glass(GlassLevel::Subtle)
                | SurfaceMaterial::Glass(GlassLevel::WindowBar)
                | SurfaceMaterial::Opaque => continue,
            };
            let Some((x, y, w, h)) = fit(b) else { continue };
            layers[n] = wire::LayerDesc {
                x,
                y,
                w,
                h,
                material,
                glass_level,
                radius: b.visual.corner_radius.top_left.0.clamp(0, 255) as u8,
                shadow_alpha: if b.visual.shadow.is_some() { 80 } else { 0 },
            };
            n += 1;
        }
        // On a change of the region count only: every full present (a keystroke's too)
        // re-declares the structure, and a line each time would put typing rhythm into the log
        // (the privacy rule).
        static LAST: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(usize::MAX);
        if LAST.swap(n, core::sync::atomic::Ordering::Relaxed) != n {
            raw_marker(&alloc::format!("apphost: submitted {n} layers"));
        }
        if dropped > 0 {
            // A silent `break` here once cost half a page its glass — the
            // cap is a wire constant, exceeding it must be visible.
            raw_marker(&alloc::format!("apphost: {dropped} glass regions over the layer cap"));
        }
        n
    }
}
