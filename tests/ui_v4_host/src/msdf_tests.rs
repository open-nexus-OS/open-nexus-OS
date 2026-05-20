// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 6b host tests — MSDF atlas generation, glyph metrics lookup, SDF sampling.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 22 tests
//!
//! TEST SCOPE:
//!   - Atlas constants (non-empty, dimensions, byte count, glyph count, metrics table size)
//!   - Glyph metrics lookup (exists, space, out-of-range, last char)
//!   - SDF sampling (center inside, corner, space all outside, out-of-range, bilinear smooth)
//!   - sdf_to_alpha (edge, deep inside, deep outside, AA width scaling)
//!   - sample_alpha convenience (inside, space transparent)
//!
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#[cfg(test)]
mod tests {
    use nexus_msdf::{
        glyph_metrics, sample_alpha, sample_atlas, sdf_to_alpha, MSDF_ATLAS, MSDF_ATLAS_HEIGHT,
        MSDF_ATLAS_WIDTH, MSDF_FIRST_CHAR, MSDF_GLYPH_COUNT, MSDF_GLYPH_SIZE, MSDF_METRICS,
    };

    // ─── Atlas constants ───

    #[test]
    fn test_atlas_non_empty() {
        assert!(!MSDF_ATLAS.is_empty(), "atlas should contain pixel data");
    }

    #[test]
    fn test_atlas_dimensions_reasonable() {
        // Layout constants should stay internally consistent with glyph cell size.
        assert_eq!(MSDF_ATLAS_WIDTH % MSDF_GLYPH_SIZE, 0);
        assert_eq!(MSDF_ATLAS_HEIGHT % MSDF_GLYPH_SIZE, 0);
    }

    #[test]
    fn test_atlas_byte_count_matches() {
        let expected = (MSDF_ATLAS_WIDTH * MSDF_ATLAS_HEIGHT * 4) as usize;
        assert_eq!(MSDF_ATLAS.len(), expected, "atlas byte count should match dimensions");
    }

    #[test]
    fn test_glyph_count_covers_printable_ascii() {
        // 32 (space) to 126 (~) = 95 glyphs
        assert_eq!(MSDF_GLYPH_COUNT, 95);
    }

    #[test]
    fn test_first_char_is_space() {
        assert_eq!(MSDF_FIRST_CHAR, 32);
    }

    #[test]
    fn test_metrics_table_size_matches() {
        assert_eq!(MSDF_METRICS.len(), MSDF_GLYPH_COUNT as usize);
    }

    // ─── Glyph metrics lookup ───

    #[test]
    fn test_glyph_metrics_a_exists() {
        let m = glyph_metrics('a').expect("'a' should be in atlas");
        assert!(m.advance > 0.0, "'a' should have positive advance");
        assert!(m.width > 0, "'a' should have positive width");
        assert!(m.height > 0, "'a' should have positive height");
    }

    #[test]
    fn test_glyph_metrics_space_exists() {
        let m = glyph_metrics(' ').expect("space should be in atlas");
        assert!(m.advance > 0.0, "space should have positive advance");
        // Space width/height may be 0
    }

    #[test]
    fn test_glyph_metrics_out_of_range_returns_none() {
        assert!(glyph_metrics('\0').is_none(), "null char outside range");
        assert!(glyph_metrics('\u{7f}').is_none(), "DEL outside range");
        assert!(glyph_metrics('é').is_none(), "accented char outside ASCII range");
    }

    #[test]
    fn test_glyph_metrics_last_char_tilde() {
        assert!(glyph_metrics('~').is_some(), "'~' should be in atlas (last printable)");
    }

    // ─── SDF sampling ───

    #[test]
    fn test_sample_atlas_i_center_inside() {
        // 'I' has no counter — center (0.5, 0.5) should be inside the glyph
        let sd = sample_atlas('I', 0.5, 0.5);
        assert!(sd > 128, "center of 'I' should be inside glyph (sd={sd})");
    }

    #[test]
    fn test_sample_atlas_a_corner_outside() {
        // Corner (0,0) should be outside 'a' glyph
        let sd = sample_atlas('a', 0.0, 0.0);
        assert!(sd < 128, "corner of 'a' should be outside glyph (sd={sd})");
    }

    #[test]
    fn test_sample_atlas_space_all_outside() {
        // Space is empty — all samples should be outside (< 128)
        for u in [0.0, 0.25, 0.5, 0.75, 1.0] {
            for v in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let sd = sample_atlas(' ', u, v);
                assert!(sd < 128, "space at ({u},{v}) should be outside (sd={sd})");
            }
        }
    }

    #[test]
    fn test_sample_atlas_out_of_range_returns_0() {
        let sd = sample_atlas('\u{7f}', 0.5, 0.5);
        assert_eq!(sd, 0, "out-of-range char should return 0");
    }

    #[test]
    fn test_sample_atlas_bilinear_smooth() {
        // Sample two adjacent UV coordinates — values should be close
        let sd1 = sample_atlas('M', 0.5, 0.5);
        let sd2 = sample_atlas('M', 0.51, 0.51);
        let diff = (sd1 as i32 - sd2 as i32).abs();
        assert!(diff < 30, "adjacent samples should be close (diff={diff})");
    }

    // ─── sdf_to_alpha ───

    #[test]
    fn test_sdf_to_alpha_edge_is_mid() {
        // At exactly the edge (128), alpha should be around half
        let alpha = sdf_to_alpha(128, 16);
        // smoothstep(112, 144, 128) → t=(128-112)=16 → 16*255/32 = 127
        assert!(alpha > 100 && alpha < 155, "edge alpha should be ~127 (got {alpha})");
    }

    #[test]
    fn test_sdf_to_alpha_deep_inside_is_opaque() {
        let alpha = sdf_to_alpha(200, 16);
        assert_eq!(alpha, 255, "deep inside should be fully opaque");
    }

    #[test]
    fn test_sdf_to_alpha_deep_outside_is_transparent() {
        let alpha = sdf_to_alpha(50, 16);
        assert_eq!(alpha, 0, "deep outside should be fully transparent");
    }

    #[test]
    fn test_sdf_to_alpha_aa_width_scales() {
        // Larger AA width → wider transition zone
        let narrow = sdf_to_alpha(140, 4);
        let wide = sdf_to_alpha(140, 16);
        // At sd=140 (12 units from edge), narrow aa (4) clamps to 255, wide aa (16) gives ~192
        assert!(
            narrow > wide || narrow == 255,
            "narrow AA should produce sharper edge (narrow={narrow}, wide={wide})"
        );
    }

    // ─── sample_alpha convenience ───

    #[test]
    fn test_sample_alpha_i_inside() {
        let alpha = sample_alpha('I', 0.5, 0.5, 16);
        assert!(alpha > 100, "center of 'I' should be at least semi-opaque (alpha={alpha})");
    }

    #[test]
    fn test_sample_alpha_space_transparent() {
        let alpha = sample_alpha(' ', 0.5, 0.5, 16);
        assert_eq!(alpha, 0, "space should be fully transparent");
    }

    // ─── GlyphMetrics struct access ───

    #[test]
    fn test_glyph_metrics_struct_fields() {
        let m = glyph_metrics('X').unwrap();
        assert!(m.atlas_col < MSDF_ATLAS_WIDTH / MSDF_GLYPH_SIZE);
        assert!(m.atlas_row < MSDF_ATLAS_HEIGHT / MSDF_GLYPH_SIZE);
        assert!(m.advance > 0.0);
        // bearing_x and bearing_y can be positive, negative, or zero
        let _ = m.bearing_x;
        let _ = m.bearing_y;
    }
}
