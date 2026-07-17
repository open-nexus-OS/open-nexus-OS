// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 6 host test suite (separable blur, shadow types, MSDF atlas, SDF shapes, 9-slice, kawase, render cache).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 98 tests (21+22+23+8+7+15+2 chain)
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

mod backdrop_tests;
mod cache_tests;
mod chain_tests;
mod kawase_tests;
mod layer_tests;
mod msdf_tests;
mod nine_slice_tests;
mod sdf_tests;
mod tile_tests;

#[cfg(test)]
mod tests {
    use nexus_effects::{blur_1d, blur_separable};
    use nexus_layout_types::{BoxShadow, Fraction, ShadowLevel, TextShadow, VisualStyle};

    // ─── blur_separable ───

    /// Identity: radius=0 produces no change.
    #[test]
    fn test_blur_separable_radius_0_identity() {
        let mut pixels = [
            255, 0, 0, 255, 0, 255, 0, 255, // row 0: red, green
            0, 0, 255, 255, 128, 128, 128, 255, // row 1: blue, gray
        ];
        let orig = pixels;
        let count = blur_separable(&mut pixels, 2, 2, 8, 0);
        assert_eq!(count, 0, "radius 0 should blur 0 pixels");
        assert_eq!(pixels, orig, "radius 0 should be identity");
    }

    /// Separable blur with radius=1 (3×3 equivalent) on a 3×3 solid-red patch.
    #[test]
    fn test_blur_separable_radius_1_solid() {
        let w: u32 = 5;
        let h: u32 = 5;
        let stride = w * 4;
        let mut pixels = vec![0u8; (stride * h) as usize];
        // Fill entire 5×5 with solid red at full opacity
        for y in 0..h {
            for x in 0..w {
                let idx = (y * stride + x * 4) as usize;
                pixels[idx] = 255;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
                pixels[idx + 3] = 255;
            }
        }
        let count = blur_separable(&mut pixels, w, h, stride, 1);
        // All pixels are the same, so box blur is identity
        assert!(count > 0, "should blur interior pixels");
        // All pixels are solid red, so blur output = input
        let center = (2 * stride + 2 * 4) as usize;
        assert_eq!(pixels[center], 255, "center red channel unchanged");
        assert_eq!(pixels[center + 3], 255, "center alpha unchanged");
        // Edge pixel also solid red with same neighbors → unchanged
        let edge = 0usize;
        assert_eq!(pixels[edge + 3], 255, "solid fill blur is identity");
    }

    /// blur_1d horizontal pass works correctly.
    #[test]
    fn test_blur_1d_horizontal_identity() {
        let mut pixels = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255];
        let orig = pixels;
        let count = blur_1d(&mut pixels, 3, 1, 12, 0, true);
        assert_eq!(count, 0);
        assert_eq!(pixels, orig);
    }

    /// blur_1d horizontal with radius 1 spreads values.
    #[test]
    fn test_blur_1d_horizontal_spread() {
        let w: u32 = 4;
        let stride = w * 4;
        let mut pixels = vec![0u8; stride as usize];
        // Single white pixel at x=0, radius=1 (window=3).
        // Sliding window: x=0 covers [0,1,2] → alpha_sum=255 → out=85.
        // x=1 covers [0,1,2] removes leftmost(255) adds rightmost(0) → alpha_sum=0.
        // x=2 covers [1,2,3] → all zero → 0.
        pixels[0] = 255;
        pixels[1] = 255;
        pixels[2] = 255;
        pixels[3] = 255;
        let count = blur_1d(&mut pixels, w, 1, stride, 1, true);
        assert!(count > 0);
        // x=0: window avg of [255,0,0] → alpha=85 (dimmed)
        assert!(pixels[3] < 255, "edge alpha should reduce");
        // x=1: window moved past the white pixel → alpha stays 0
        assert_eq!(pixels[7], 0, "x=1 outside window range stays 0");
    }

    // ─── BoxShadow / ShadowLevel ───

    #[test]
    fn test_box_shadow_default_values() {
        let shadow = BoxShadow::default();
        assert_eq!(shadow.offset_x.0, 0);
        assert_eq!(shadow.offset_y.0, 4);
        assert_eq!(shadow.blur_radius.0, 8);
        assert_eq!(shadow.spread.0, 0);
        assert_eq!(shadow.color.a, 64);
    }

    #[test]
    fn test_text_shadow_default_values() {
        let shadow = TextShadow::default();
        assert_eq!(shadow.offset_x.0, 0);
        assert_eq!(shadow.offset_y.0, 2);
        assert_eq!(shadow.blur_radius.0, 4);
        assert_eq!(shadow.color.a, 80);
    }

    #[test]
    fn test_shadow_level_sm_to_box_shadow() {
        let shadow = ShadowLevel::Sm.to_box_shadow();
        assert_eq!(shadow.blur_radius.0, 2);
        assert_eq!(shadow.offset_y.0, 1);
        assert_eq!(shadow.color.a, 31);
    }

    #[test]
    fn test_shadow_level_md_to_box_shadow() {
        // Design-handoff elevation scale: md 0 4 12 .15 (see layout-types border.rs).
        let shadow = ShadowLevel::Md.to_box_shadow();
        assert_eq!(shadow.blur_radius.0, 12);
        assert_eq!(shadow.offset_y.0, 4);
        assert_eq!(shadow.color.a, 38);
    }

    #[test]
    fn test_shadow_level_lg_to_box_shadow() {
        let shadow = ShadowLevel::Lg.to_box_shadow();
        assert_eq!(shadow.blur_radius.0, 24);
        assert_eq!(shadow.offset_y.0, 8);
    }

    #[test]
    fn test_shadow_level_xl_to_box_shadow() {
        let shadow = ShadowLevel::Xl.to_box_shadow();
        assert_eq!(shadow.blur_radius.0, 32);
        assert_eq!(shadow.offset_y.0, 12);
    }

    #[test]
    fn test_shadow_level_xxl2_to_box_shadow() {
        let shadow = ShadowLevel::Xxl2.to_box_shadow();
        assert_eq!(shadow.blur_radius.0, 50);
        assert_eq!(shadow.offset_y.0, 25);
        assert_eq!(shadow.spread.0, 0);
    }

    #[test]
    fn test_shadow_level_default_is_md() {
        let default = ShadowLevel::default();
        assert_eq!(default, ShadowLevel::Md);
    }

    // ─── VisualStyle extensions ───

    #[test]
    fn test_visual_style_default_has_no_shadow() {
        let style = VisualStyle::default();
        assert!(style.shadow.is_none());
        assert!(style.text_shadow.is_none());
    }

    #[test]
    fn test_visual_style_with_box_shadow() {
        let shadow = BoxShadow::default();
        let style = VisualStyle { shadow: Some(shadow), ..Default::default() };
        assert!(style.shadow.is_some());
        assert_eq!(style.shadow.unwrap().blur_radius.0, 8);
    }

    #[test]
    fn test_visual_style_with_text_shadow() {
        let shadow = TextShadow::default();
        let style = VisualStyle { text_shadow: Some(shadow), ..Default::default() };
        assert!(style.text_shadow.is_some());
        assert_eq!(style.text_shadow.unwrap().blur_radius.0, 4);
    }

    #[test]
    fn test_visual_style_opacity_default_is_none() {
        let style = VisualStyle::default();
        assert!(style.opacity.is_none());
    }

    #[test]
    fn test_visual_style_opacity_some() {
        let style = VisualStyle { opacity: Some(Fraction::new(128)), ..Default::default() };
        assert_eq!(style.opacity.unwrap().as_u8(), 128);
    }

    // ─── Fraction ───

    #[test]
    fn test_fraction_opaque() {
        assert_eq!(Fraction::OPAQUE.0, 255);
        assert_eq!(Fraction::OPAQUE.as_u8(), 255);
    }

    #[test]
    fn test_fraction_transparent() {
        assert_eq!(Fraction::TRANSPARENT.0, 0);
        assert_eq!(Fraction::TRANSPARENT.as_u8(), 0);
    }

    #[test]
    fn test_fraction_new_clamps() {
        assert_eq!(Fraction::new(300).0, 255);
        assert_eq!(Fraction::new(128).0, 128);
        assert_eq!(Fraction::new(0).0, 0);
    }

    #[test]
    fn test_fraction_blend_factor() {
        let f = Fraction::new(128);
        assert_eq!(f.blend_factor(), (128, 256));
        let opaque = Fraction::OPAQUE;
        assert_eq!(opaque.blend_factor(), (255, 256));
        let transparent = Fraction::TRANSPARENT;
        assert_eq!(transparent.blend_factor(), (0, 256));
    }
}
