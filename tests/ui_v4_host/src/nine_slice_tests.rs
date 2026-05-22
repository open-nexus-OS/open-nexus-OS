// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 6d host tests — 9-slice shadow compositing (corners, edges, fill, cache).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 8 tests
//!
//! TEST SCOPE:
//!   - Basic output (produces output, zero-size noop, budget exhausted)
//!   - Corners blurred (blur_radius > 0 spreads alpha)
//!   - Center fill (solid shadow alpha at element center)
//!   - Cache (hit, different params → different keys)
//!   - Comparison vs full-surface blur (area ratio non-trivial)
//!
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#[cfg(test)]
mod tests {
    use nexus_effects::{
        composite_drop_shadow, composite_nine_slice_shadow, DropShadowParams, EffectBudget,
        EffectCache, NineSliceCompositeParams, NineSliceShadow,
    };
    use nexus_layout_types::Rgba8;

    fn make_target(w: u32, h: u32) -> (Vec<u8>, u32) {
        let stride = w * 4;
        (vec![0u8; (stride * h) as usize], stride)
    }

    fn black_shadow() -> Rgba8 {
        Rgba8 {
            r: 0,
            g: 0,
            b: 0,
            a: 128,
        }
    }

    // ─── basic 9-slice rendering ───

    #[test]
    fn test_nine_slice_produces_output() {
        let elem_w: u32 = 100;
        let elem_h: u32 = 80;
        let (mut target, stride) = make_target(200, 200);
        let shadow = NineSliceShadow {
            corner_size: 12,
            blur_radius: 4,
            spread: 2,
            color: black_shadow(),
        };
        let mut budget = EffectBudget::default();
        let count = composite_nine_slice_shadow(
            &mut target,
            &shadow,
            NineSliceCompositeParams {
                target_w: 200,
                target_h: 200,
                stride,
                elem_w,
                elem_h,
                offset_x: 50,
                offset_y: 60,
            },
            &mut budget,
            None,
        );
        assert!(count > 0, "should composite some shadow pixels");
    }

    #[test]
    fn test_nine_slice_zero_size_noop() {
        let (mut target, stride) = make_target(100, 100);
        let shadow = NineSliceShadow {
            corner_size: 8,
            blur_radius: 2,
            spread: -50, // shrinks to zero
            color: black_shadow(),
        };
        let mut budget = EffectBudget::default();
        let count = composite_nine_slice_shadow(
            &mut target,
            &shadow,
            NineSliceCompositeParams {
                target_w: 100,
                target_h: 100,
                stride,
                elem_w: 10,
                elem_h: 10,
                offset_x: 0,
                offset_y: 0,
            },
            &mut budget,
            None,
        );
        assert_eq!(count, 0, "zero-size shadow should produce no output");
    }

    #[test]
    fn test_nine_slice_budget_exhausted() {
        let (mut target, stride) = make_target(200, 200);
        let shadow = NineSliceShadow {
            corner_size: 12,
            blur_radius: 4,
            spread: 0,
            color: black_shadow(),
        };
        let mut budget = EffectBudget::new(0); // zero budget
        let count = composite_nine_slice_shadow(
            &mut target,
            &shadow,
            NineSliceCompositeParams {
                target_w: 200,
                target_h: 200,
                stride,
                elem_w: 100,
                elem_h: 100,
                offset_x: 0,
                offset_y: 0,
            },
            &mut budget,
            None,
        );
        assert_eq!(count, 0, "exhausted budget should skip");
    }

    // ─── corners are blurred ───

    #[test]
    fn test_nine_slice_corners_blurred() {
        let elem_w: u32 = 60;
        let elem_h: u32 = 60;
        let (mut target, stride) = make_target(120, 120);
        // Blur radius > 0 should soften corners
        let blurred = NineSliceShadow {
            corner_size: 10,
            blur_radius: 4,
            spread: 0,
            color: black_shadow(),
        };
        let mut budget = EffectBudget::default();
        let _ = composite_nine_slice_shadow(
            &mut target,
            &blurred,
            NineSliceCompositeParams {
                target_w: 120,
                target_h: 120,
                stride,
                elem_w,
                elem_h,
                offset_x: 30,
                offset_y: 30,
            },
            &mut budget,
            None,
        );
        // After blur, corner regions should have non-max alpha (spread out)
        let corner_idx = (30 * stride + 30 * 4) as usize;
        assert!(target[corner_idx + 3] > 0, "corner should have some alpha");
    }

    // ─── fill is solid ───

    #[test]
    fn test_nine_slice_center_filled() {
        let elem_w: u32 = 100;
        let elem_h: u32 = 100;
        let (mut target, stride) = make_target(200, 200);
        let shadow = NineSliceShadow {
            corner_size: 12,
            blur_radius: 0,
            spread: 0,
            color: black_shadow(),
        };
        let mut budget = EffectBudget::default();
        let _ = composite_nine_slice_shadow(
            &mut target,
            &shadow,
            NineSliceCompositeParams {
                target_w: 200,
                target_h: 200,
                stride,
                elem_w,
                elem_h,
                offset_x: 50,
                offset_y: 50,
            },
            &mut budget,
            None,
        );
        // Center of shadow region (elem center + offset) should have solid shadow alpha
        let cx = 50 + elem_w as i32 / 2;
        let cy = 50 + elem_h as i32 / 2;
        let idx = (cy as usize) * stride as usize + (cx as usize) * 4;
        assert_eq!(
            target[idx + 3],
            shadow.color.a,
            "center fill should be solid shadow alpha"
        );
    }

    // ─── cache ───

    #[test]
    fn test_nine_slice_cache_hit() {
        let elem_w: u32 = 60;
        let elem_h: u32 = 40;
        let (mut target1, stride) = make_target(120, 120);
        let (mut target2, _) = make_target(120, 120);
        let shadow = NineSliceShadow {
            corner_size: 10,
            blur_radius: 3,
            spread: 1,
            color: black_shadow(),
        };
        let mut budget = EffectBudget::default();
        let mut cache = EffectCache::with_capacity(16);

        let c1 = composite_nine_slice_shadow(
            &mut target1,
            &shadow,
            NineSliceCompositeParams {
                target_w: 120,
                target_h: 120,
                stride,
                elem_w,
                elem_h,
                offset_x: 30,
                offset_y: 40,
            },
            &mut budget,
            Some(&mut cache),
        );
        assert!(c1 > 0);
        assert_eq!(
            cache.len(),
            1,
            "cache should have one entry after first render"
        );

        let c2 = composite_nine_slice_shadow(
            &mut target2,
            &shadow,
            NineSliceCompositeParams {
                target_w: 120,
                target_h: 120,
                stride,
                elem_w,
                elem_h,
                offset_x: 30,
                offset_y: 40,
            },
            &mut budget,
            Some(&mut cache),
        );
        assert!(c2 > 0);
        assert_eq!(cache.len(), 1, "cache should still have one entry (hit)");
        assert_eq!(target1, target2, "cached result should be identical");
    }

    #[test]
    fn test_nine_slice_different_params_different_cache_key() {
        let (mut target1, stride) = make_target(120, 120);
        let (mut target2, _) = make_target(120, 120);
        let shadow1 = NineSliceShadow {
            corner_size: 8,
            blur_radius: 2,
            spread: 0,
            color: black_shadow(),
        };
        let shadow2 = NineSliceShadow {
            corner_size: 10,
            blur_radius: 4,
            spread: 0,
            color: black_shadow(),
        };
        let mut budget = EffectBudget::default();
        let mut cache = EffectCache::with_capacity(16);

        let _ = composite_nine_slice_shadow(
            &mut target1,
            &shadow1,
            NineSliceCompositeParams {
                target_w: 120,
                target_h: 120,
                stride,
                elem_w: 50,
                elem_h: 50,
                offset_x: 35,
                offset_y: 35,
            },
            &mut budget,
            Some(&mut cache),
        );
        let _ = composite_nine_slice_shadow(
            &mut target2,
            &shadow2,
            NineSliceCompositeParams {
                target_w: 120,
                target_h: 120,
                stride,
                elem_w: 50,
                elem_h: 50,
                offset_x: 35,
                offset_y: 35,
            },
            &mut budget,
            Some(&mut cache),
        );
        assert_eq!(
            cache.len(),
            2,
            "different params should be separate cache entries"
        );
    }

    // ─── comparison with full-surface blur ───

    #[test]
    fn test_nine_slice_vs_full_blur_similar() {
        let elem_w: u32 = 80;
        let elem_h: u32 = 60;
        let (mut target_nine, stride) = make_target(160, 160);
        let (mut target_full, _) = make_target(160, 160);

        let shadow = NineSliceShadow {
            corner_size: 10,
            blur_radius: 3,
            spread: 0,
            color: black_shadow(),
        };
        let mut budget = EffectBudget::default();

        let _ = composite_nine_slice_shadow(
            &mut target_nine,
            &shadow,
            NineSliceCompositeParams {
                target_w: 160,
                target_h: 160,
                stride,
                elem_w,
                elem_h,
                offset_x: 40,
                offset_y: 50,
            },
            &mut budget,
            None,
        );

        // Full-surface version: use composite_drop_shadow with same alpha mask
        // Alpha mask must cover full target area; only the element region is opaque
        let mut alpha = vec![0u8; (160 * 160) as usize];
        for y in 0..elem_h {
            for x in 0..elem_w {
                alpha[(y * 160 + x) as usize] = 255;
            }
        }
        let mut budget2 = EffectBudget::default();
        let _ = composite_drop_shadow(
            &mut target_full,
            &alpha,
            DropShadowParams {
                width: 160,
                height: 160,
                stride,
                offset_x: 0,
                offset_y: 0,
                shadow_color: black_shadow(),
            },
            &mut budget2,
        );

        // Both should have non-zero output in roughly the same region
        let mut nine_pixels = 0u32;
        let mut full_pixels = 0u32;
        for i in (3..target_nine.len()).step_by(4) {
            if target_nine[i] > 0 {
                nine_pixels += 1;
            }
            if target_full[i] > 0 {
                full_pixels += 1;
            }
        }
        assert!(nine_pixels > 0);
        assert!(full_pixels > 0);
        // 9-slice should produce fewer pixels (corners+edges+fill) than full blur
        // but shouldn't be drastically different
        let ratio = nine_pixels as f32 / full_pixels as f32;
        assert!(
            ratio > 0.3,
            "9-slice should cover reasonable portion of full blur area (ratio={ratio:.2})"
        );
    }
}
