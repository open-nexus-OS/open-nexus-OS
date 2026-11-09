// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` material regions — the region list of the surface's structure
//! (`OP_SURFACE_TREE`, RFC-0109; the R1 seam): split out of `interaction.rs` (module-size
//! ratchet) — the hit-testing stays there, the compositor-region seam lives here.

use super::*;

impl super::DslApp {
    /// The material-tagged regions of the current layout (the R1 seam, RFC-0116): a blur
    /// surface on the wallpaper (`panel`, `window`, `dock`, `thin`, `ultraThin`) becomes a glass
    /// `LayerDesc` of its backdrop's bucket — windowd composites it as a real frosted
    /// `nexus-gfx` layer — and an opaque surface (`pane`, `overlay`) an occluder; a `card` is
    /// painted into the surface and is no region. They ride the
    /// surface's structure (`band::send_tree`), so a re-created surface declares them anew with
    /// its first present — and windowd clears them at create, so neither side can carry a stale
    /// set alone. A box of a band's content is no region: it moves with its layer, its tint is in
    /// the band. A box an animation moves declares its region where it is painted now (its
    /// translate and scale) — a blur never sits where the glass it frosts is not (the greeter's
    /// entry); a fade keeps it (the material is there, its tint fades). Returns the count
    /// written into `out`; `nodes[i]` is region `i`'s node (a motion layer claims the regions of
    /// its subtree, TASK-0354 P2).
    pub(super) fn regions(
        &self,
        out: &mut [wire::LayerDesc; wire::MAX_SURFACE_LAYERS],
        nodes: &mut [usize; wire::MAX_SURFACE_LAYERS],
    ) -> usize {
        let mut expanded =
            [nexus_scene_raster::NodeAnim::identity(0); super::anim::MAX_EXPANDED_ANIMS];
        let k = self.expand_node_anims(&mut expanded);
        let anims = &expanded[..k];
        // The region inside the base layer (windowd refuses a structure with one outside it).
        let (sw, sh) = (self.w as i32, self.h as i32);
        let fit = |b: &nexus_layout::LayoutBox| {
            let (x, y, w, h) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
            let (x, y, w, h) = match anims.iter().find(|a| a.node_id == b.node_id) {
                Some(a) => a.transform_rect(x, y, w, h),
                None => (x, y, w, h),
            };
            let (x0, y0) = (x.max(0), y.max(0));
            let (x1, y1) = ((x + w).min(sw), (y + h).min(sh));
            (x1 > x0 && y1 > y0).then(|| (x0 as u16, y0 as u16, (x1 - x0) as u16, (y1 - y0) as u16))
        };
        let layers = out;
        let mut n = 0;
        let mut dropped = 0usize;
        for b in &self.layout.boxes {
            if self.in_band(b) {
                continue;
            }
            let Some(material) = b.visual.material else { continue };
            let full = b.visual.opacity.is_none_or(|o| o.0 >= 255);
            if wire::theme_is_solid(self.theme_mode) {
                // Solid materials (TASK-0337): every material box is painted at its fill's full
                // opacity — an opaque region (an occluder) for a root, never a blur.
                if b.glass_nested || !full || material == nexus_layout_types::Material::Card {
                    continue;
                }
                if n >= wire::MAX_SURFACE_LAYERS {
                    dropped += 1;
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
                nodes[n] = b.node_id;
                n += 1;
                continue;
            }
            let Some((material, glass_level)) = region_kind(material, b.glass_nested, full) else {
                continue;
            };
            if n >= wire::MAX_SURFACE_LAYERS {
                dropped += 1;
                continue;
            }
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
            nodes[n] = b.node_id;
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

/// The region a material box declares (RFC-0116), if any: `(MATERIAL_*, GLASS_*)`.
///
/// A blur surface is a glass region of its backdrop's bucket — only as a ROOT: one blur per
/// stack, and a nested one blends into its parent's pixels. An opaque surface drawn at full
/// opacity is an occluder wherever it sits (a pane occludes the window's blur, a menu a
/// panel's); faded, it shows what lies below and must not claim to hide it. A card is painted,
/// never a region.
pub(super) fn region_kind(
    material: nexus_layout_types::Material,
    nested: bool,
    full: bool,
) -> Option<(u8, u8)> {
    use nexus_layout_types::Material;
    let glass = |level| (!nested).then_some((wire::MATERIAL_GLASS, level));
    match material {
        Material::Panel => glass(wire::GLASS_PANEL),
        Material::Window => glass(wire::GLASS_WINDOW),
        Material::Dock => glass(wire::GLASS_DOCK),
        Material::Thin => glass(wire::GLASS_THIN),
        Material::UltraThin => glass(wire::GLASS_ULTRA_THIN),
        Material::Pane | Material::Overlay => full.then_some((wire::MATERIAL_OPAQUE, 0)),
        Material::Card => None,
    }
}
