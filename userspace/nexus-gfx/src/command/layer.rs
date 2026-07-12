// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The compositor layer SSOT. A `Layer` declares one GPU-composited
//! surface with its per-layer effects (opacity, corner radius, drop shadow,
//! frosted backdrop). `RenderCommandEncoder::composite_layer_full` emits the
//! one canonical command sequence for it — restore-backdrop → blur → composite
//! — replacing the recipe that compositors otherwise hand-roll per element.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: command-stream assertions in this module's `tests`

/// A soft drop shadow cast behind a layer (rendered by the composite shader as
/// an SDF falloff over `blur` px). `offset_y` shifts the shadow down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerShadow {
    pub blur: u32,
    pub offset_y: i32,
    pub alpha: u32,
}

/// How a glass layer's blurred backdrop is cached across frames — the three
/// blur strategies a real compositor uses:
///
/// - `None`: re-blur the live backdrop every frame (animating position, or a
///   backdrop that changes underneath — chrome panels).
/// - `Write`: blur this frame and persist into the cache surface (first settled
///   frame of a window).
/// - `Read`: reuse the persisted blur, skipping the per-frame gaussian.
///
/// `display_row_offset` maps a display y to its absolute VMO row for the cache
/// transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackdropCache {
    None,
    Write { cache_x: u32, cache_row_abs: u32, display_row_offset: u32 },
    Read { cache_x: u32, cache_row_abs: u32, display_row_offset: u32 },
}

/// The frosted backdrop effect behind a glass layer: how strongly to blur and
/// saturate the content behind it, where the clean backdrop is restored from,
/// and how it is cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerBackdrop {
    pub blur_radius: u32,
    pub saturation_percent: u32,
    /// Grow the restore blit by this many px on every side so a soft drop shadow
    /// blends over a clean backdrop and never trails. The blur + cache still cover
    /// only the layer rect. 0 = restore the layer rect exactly (chrome panels with
    /// no shadow halo to keep clean). When > 0 the halo pad is restored even on a
    /// `Read` (cached) frame, since the cache only repaints the layer rect.
    pub restore_halo_pad: u32,
    /// Where the clean (un-blurred) backdrop lives in the retained plane: a glass
    /// layer at display `y` restores from `y + retained_src_y_offset`.
    pub retained_src_y_offset: u32,
    pub cache: BackdropCache,
}

/// One GPU-composited layer: a content surface placed on the display with
/// per-layer effects. Geometry is expected pre-clamped to the display by the
/// caller (visibility/bounds are compositor policy, not the emitter's job).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    /// Source content surface: absolute VMO row + packed column origin.
    pub src_row_abs: u32,
    pub src_x: u32,
    pub width: u32,
    pub height: u32,
    /// Content sub-size drawn at the layer's top-left (`0` = same as
    /// `width`/`height`). When the content is smaller than the layer, the
    /// backdrop blur + rounding cover `width`×`height` (the frame) but the source
    /// texture is drawn at `content_w`×`content_h` (the band); the rest is blurred
    /// backdrop only — the "glass frame grows, content 1:1 top-left" resize path.
    pub content_w: u32,
    pub content_h: u32,
    /// Destination on the display plane.
    pub dst_x: u32,
    pub dst_y: u32,
    pub opacity: u32,
    pub corner_radius: u32,
    /// Scroll identity (`0` = not scrollable): the backend re-samples the layer
    /// at the id's source-row override on a lightweight scroll command (GPU
    /// scroll fast path).
    pub scroll_id: u32,
    /// WebRender scroll band: the FULL resident-content band the compositor must
    /// upload to the GPU atlas texture ONCE so the `src_row` override can shift
    /// WITHIN it (`0` = not scrollable, upload only the visible rows). Without
    /// this the backend uploads only `height` rows at the current source row, so
    /// a shifted `src_row` samples never-uploaded rows (the body never scrolls).
    /// `scroll_band_top_abs` = absolute atlas row of the band top;
    /// `scroll_band_h` = band height (rows). The SAMPLE still uses `src_row_abs`
    /// + `height` (the overridden scroll position) — only the UPLOAD region grows.
    pub scroll_band_top_abs: u32,
    pub scroll_band_h: u32,
    pub shadow: Option<LayerShadow>,
    pub backdrop: Option<LayerBackdrop>,
}

impl Layer {
    /// A plain opaque layer: no shadow, no backdrop, full opacity, square corners.
    pub fn opaque(src_row_abs: u32, src_x: u32, width: u32, height: u32, dst_x: u32, dst_y: u32) -> Self {
        Self {
            src_row_abs,
            src_x,
            width,
            height,
            content_w: 0,
            content_h: 0,
            dst_x,
            dst_y,
            opacity: 255,
            corner_radius: 0,
            scroll_id: 0,
            scroll_band_top_abs: 0,
            scroll_band_h: 0,
            shadow: None,
            backdrop: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::buffer::{Command, CommandBuffer, RgbaColor};
    use crate::core::types::RenderPassDesc;

    fn emit(layer: &Layer) -> Vec<Command> {
        let mut cb = CommandBuffer::new();
        {
            let mut enc = cb
                .begin_render_pass(RenderPassDesc { color_attachments: alloc::vec![], width: 1280, height: 800 });
            enc.composite_layer_full(layer, (1280, 800)).expect("emit");
        }
        cb.commands.clone()
    }

    // A bare opaque layer emits exactly one CompositeLayer, no restore/blur.
    #[test]
    fn opaque_layer_is_a_single_composite() {
        let cmds = emit(&Layer::opaque(100, 0, 200, 64, 40, 50));
        assert_eq!(cmds.len(), 1);
        assert_eq!(
            cmds[0],
            Command::CompositeLayer {
                src_row_abs: 100,
                src_x: 0,
                width: 200,
                height: 64,
                dst_x: 40,
                dst_y: 50,
                opacity: 255,
                corner_radius: 0,
                shadow_blur: 0,
                shadow_offset_y: 0,
                shadow_alpha: 0,
                backdrop_blur: 0,
                scroll_id: 0,
                content_w: 0,
                content_h: 0,
                scroll_band_top_abs: 0,
                scroll_band_h: 0,
            }
        );
    }

    // A glass layer with a fresh blur: restore from the retained plane, blur in
    // place, then composite. The composite's own backdrop_blur stays 0 (the blur
    // is the explicit BlurBackdrop above; keeps layer text sharp).
    #[test]
    fn fresh_glass_emits_restore_blur_composite() {
        let layer = Layer {
            shadow: Some(LayerShadow { blur: 10, offset_y: 4, alpha: 80 }),
            backdrop: Some(LayerBackdrop {
                blur_radius: 20,
                saturation_percent: 140,
                restore_halo_pad: 0,
                retained_src_y_offset: 2048,
                cache: BackdropCache::None,
            }),
            corner_radius: 16,
            ..Layer::opaque(300, 0, 256, 128, 60, 70)
        };
        let cmds = emit(&layer);
        assert_eq!(cmds.len(), 3);
        assert!(matches!(
            cmds[0],
            Command::BlitSurface { src_x: 60, src_y: 2118, dst_x: 60, dst_y: 70, width: 256, height: 128 }
        ));
        assert!(matches!(
            cmds[1],
            Command::BlurBackdrop { radius: 20, saturation_percent: 140, .. }
        ));
        assert!(matches!(
            cmds[2],
            Command::CompositeLayer {
                corner_radius: 16,
                shadow_blur: 10,
                shadow_offset_y: 4,
                shadow_alpha: 80,
                backdrop_blur: 20,
                ..
            }
        ));
    }

    // First settled frame: fresh blur, then persist into the cache surface.
    #[test]
    fn fresh_caching_glass_persists_the_blur() {
        let layer = Layer {
            backdrop: Some(LayerBackdrop {
                blur_radius: 20,
                saturation_percent: 140,
                restore_halo_pad: 0,
                retained_src_y_offset: 2048,
                cache: BackdropCache::Write { cache_x: 512, cache_row_abs: 3000, display_row_offset: 1024 },
            }),
            ..Layer::opaque(300, 0, 256, 128, 60, 70)
        };
        let cmds = emit(&layer);
        assert_eq!(cmds.len(), 4);
        assert!(matches!(cmds[0], Command::BlitSurface { .. }));
        assert!(matches!(cmds[1], Command::BlurBackdrop { .. }));
        // Persist the blurred backdrop: display(60, 1024+70) -> cache(512, 3000).
        assert!(matches!(
            cmds[2],
            Command::BlitAbsolute {
                src_x: 60,
                src_y_abs: 1094,
                dst_x: 512,
                dst_y_abs: 3000,
                width: 256,
                height: 128
            }
        ));
        assert!(matches!(cmds[3], Command::CompositeLayer { .. }));
    }

    // Settled reuse: blit the cached blur back, no per-frame BlurBackdrop.
    #[test]
    fn cached_glass_skips_the_blur() {
        let layer = Layer {
            backdrop: Some(LayerBackdrop {
                blur_radius: 20,
                saturation_percent: 140,
                restore_halo_pad: 0,
                retained_src_y_offset: 2048,
                cache: BackdropCache::Read { cache_x: 512, cache_row_abs: 3000, display_row_offset: 1024 },
            }),
            ..Layer::opaque(300, 0, 256, 128, 60, 70)
        };
        let cmds = emit(&layer);
        assert_eq!(cmds.len(), 2);
        // Restore cached blur: cache(512, 3000) -> display(60, 1024+70).
        assert!(matches!(
            cmds[0],
            Command::BlitAbsolute {
                src_x: 512,
                src_y_abs: 3000,
                dst_x: 60,
                dst_y_abs: 1094,
                width: 256,
                height: 128
            }
        ));
        assert!(!cmds.iter().any(|c| matches!(c, Command::BlurBackdrop { .. })));
        assert!(matches!(cmds[1], Command::CompositeLayer { .. }));
    }

    // A shadowed window restores a padded halo (window + shadow pad) from the
    // retained plane, but blurs only the window rect. The halo clamps to bounds.
    #[test]
    fn restore_halo_pads_the_restore_but_not_the_blur() {
        let layer = Layer {
            shadow: Some(LayerShadow { blur: 24, offset_y: 8, alpha: 80 }),
            backdrop: Some(LayerBackdrop {
                blur_radius: 20,
                saturation_percent: 140,
                restore_halo_pad: 32,
                retained_src_y_offset: 800,
                cache: BackdropCache::None,
            }),
            ..Layer::opaque(500, 0, 400, 300, 100, 120)
        };
        let cmds = emit(&layer);
        // Restore covers (100-32, 120-32) sized (400+64, 300+64): a padded halo.
        assert!(matches!(
            cmds[0],
            Command::BlitSurface { src_x: 68, src_y: 888, dst_x: 68, dst_y: 88, width: 464, height: 364 }
        ));
        // Blur covers ONLY the window rect (100,120,400,300), not the halo.
        assert!(matches!(
            cmds[1],
            Command::BlurBackdrop { rect: crate::core::types::TileRect { x: 100, y: 120, width: 400, height: 300 }, .. }
        ));
    }

    // Cached window WITH a shadow halo (chat): even though the blur is cached, the
    // halo pad is restored from the retained plane first so the shadow stays clean,
    // THEN the cached blur repaints the window rect.
    #[test]
    fn cached_with_halo_restores_pad_then_reads_cache() {
        let layer = Layer {
            shadow: Some(LayerShadow { blur: 24, offset_y: 8, alpha: 80 }),
            backdrop: Some(LayerBackdrop {
                blur_radius: 20,
                saturation_percent: 140,
                restore_halo_pad: 32,
                retained_src_y_offset: 800,
                cache: BackdropCache::Read { cache_x: 700, cache_row_abs: 3200, display_row_offset: 1600 },
            }),
            ..Layer::opaque(500, 0, 400, 300, 100, 120)
        };
        let cmds = emit(&layer);
        assert_eq!(cmds.len(), 3);
        // Halo restore (pad) from the retained plane.
        assert!(matches!(
            cmds[0],
            Command::BlitSurface { src_x: 68, src_y: 888, dst_x: 68, dst_y: 88, width: 464, height: 364 }
        ));
        // Cached blur repaints only the window rect.
        assert!(matches!(
            cmds[1],
            Command::BlitAbsolute { src_x: 700, src_y_abs: 3200, dst_x: 100, dst_y_abs: 1720, width: 400, height: 300 }
        ));
        assert!(!cmds.iter().any(|c| matches!(c, Command::BlurBackdrop { .. })));
        assert!(matches!(cmds[2], Command::CompositeLayer { .. }));
    }

    // A scrollable layer tags the composite with its id so the backend retains it.
    #[test]
    fn scroll_id_tags_the_composite() {
        let layer = Layer { scroll_id: 3, ..Layer::opaque(100, 0, 200, 64, 40, 50) };
        let cmds = emit(&layer);
        assert!(matches!(cmds[0], Command::CompositeLayer { scroll_id: 3, .. }));
    }

    #[test]
    fn rgba_color_unused_import_guard() {
        // Keep RgbaColor import meaningful if future shadow color is parameterized.
        let _ = RgbaColor::new(0, 0, 0, 80);
    }
}
