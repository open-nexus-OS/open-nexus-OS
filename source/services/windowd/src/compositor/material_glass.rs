// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: One app-declared material-glass REGION composited through the
//! `nexus-gfx` layer SSOT (RFC-0067 Revival R1) — the per-region seam split
//! out of `shell_window.rs` (module-size ratchet): whole-window glass stays
//! there, the region recipe lives here. Re-exported through `shell_window`
//! so the scene encoder's call sites read unchanged.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: driven by `runtime/scene.rs` encode paths + QEMU smoke.

use crate::compositor::{DARK_GLASS_SATURATION_PERCENT, RETAINED_ROW_OFFSET};
use nexus_gfx::{BackdropCache, Layer, LayerBackdrop, LayerShadow, RenderCommandEncoder};

/// Parameters for one **material-tagged glass region** of a client surface (R1
/// layer seam). Unlike [`GlassCompositeParams`] (a whole window), this composites
/// an arbitrary sub-rect the app declared as glass, sampling its content from the
/// app surface's atlas band and blurring the retained backdrop behind it.
#[derive(Clone, Copy)]
pub(crate) struct MaterialLayerParams {
    pub src_row_abs: u32,
    pub src_x: u32,
    pub width: u32,
    pub height: u32,
    pub dst_x: u32,
    pub dst_y: u32,
    pub corner_radius: u32,
    pub shadow_alpha: u32,
    /// Backdrop blur radius (from the glass level: panel/card/subtle/window).
    pub blur_radius: u32,
}

/// Composite one app-declared glass region through the `nexus-gfx` layer SSOT —
/// the same recipe as [`ShellWindow::composite_glass`], per region, with the
/// backdrop re-blurred live each present (`BackdropCache::None`; a per-region
/// blur cache is a later optimization). This is how the shell's topbar/dock/cards
/// become real frosted layers over the wallpaper (RFC-0067 Revival R1).
pub(crate) fn composite_material_glass(
    encoder: &mut RenderCommandEncoder<'_>,
    p: MaterialLayerParams,
    mode_w: u32,
    mode_h: u32,
) {
    if p.width == 0 || p.height == 0 || p.dst_x >= mode_w || p.dst_y >= mode_h {
        return;
    }
    let w = p.width.min(mode_w.saturating_sub(p.dst_x));
    let h = p.height.min(mode_h.saturating_sub(p.dst_y));
    let _ = encoder.composite_layer_full(
        &Layer {
            src_row_abs: p.src_row_abs,
            src_x: p.src_x,
            width: w,
            height: h,
            content_w: 0,
            content_h: 0,
            dst_x: p.dst_x,
            dst_y: p.dst_y,
            opacity: 255,
            corner_radius: p.corner_radius,
            scroll_id: 0,
            // Untagged: material glass rides the app's DESKTOP surface.
            layer_id: 0,
            content_epoch: crate::atlas::atlas_content_epoch(),
            scroll_band_top_abs: 0,
            scroll_band_h: 0,
            shadow: (p.shadow_alpha > 0).then_some(LayerShadow {
                blur: 24,
                offset_y: 8,
                alpha: p.shadow_alpha,
            }),
            backdrop: Some(LayerBackdrop {
                blur_radius: p.blur_radius,
                saturation_percent: DARK_GLASS_SATURATION_PERCENT,
                restore_halo_pad: 0,
                retained_src_y_offset: RETAINED_ROW_OFFSET,
                cache: BackdropCache::None,
            }),
        },
        (mode_w, mode_h),
    );
}
