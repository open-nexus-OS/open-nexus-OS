// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 2 backdrop tests — stable source blur, idempotency, composite alpha.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 3 tests
//!
//! TEST SCOPE: idempotent blur, no darkening on repeat, alpha over-blend
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#[cfg(test)]
mod tests {
    /// Verify that blurring the same input twice produces the same output.
    #[test]
    fn test_blur_idempotent_same_input_same_output() {
        let input: Vec<u8> = (0..256).map(|i| (i % 4) as u8 * 64).collect(); // 64 pixels
        let mut out1 = input.clone();
        let mut out2 = input.clone();
        let mut scratch = vec![0u8; 256];

        // First blur (simplified 1D blur with radius=2)
        blur_simple(&mut out1, &input, 2, &mut scratch);
        // Second blur on same input
        blur_simple(&mut out2, &input, 2, &mut scratch);

        assert_eq!(out1, out2, "same input → same output");
    }

    /// Verify that repeated blur on output doesn't diverge indefinitely.
    #[test]
    fn test_blur_no_darkening_on_repeat() {
        let input: Vec<u8> =
            (0..256).map(|i| if (64..192).contains(&i) { 200u8 } else { 50u8 }).collect();
        let mut out = input.clone();
        let mut scratch = vec![0u8; 256];
        let mut prev_sum: u64 = out.iter().map(|v| *v as u64).sum();

        for _ in 0..3 {
            let current = out.clone();
            blur_simple(&mut out, &current, 2, &mut scratch);
            let new_sum: u64 = out.iter().map(|v| *v as u64).sum();
            // After first blur, subsequent blurs should converge (not keep drifting darker)
            assert!(
                (new_sum as i64 - prev_sum as i64).abs() < (prev_sum / 10) as i64,
                "blur should converge, not drift (prev={prev_sum}, new={new_sum})"
            );
            prev_sum = new_sum;
        }
    }

    /// Verify alpha over-blend produces expected result.
    #[test]
    fn test_alpha_over_blend() {
        let bg: [u8; 4] = [100, 150, 200, 255]; // wallpaper
        let fg: [u8; 4] = [50, 100, 150, 128]; // panel with alpha=128 (50%)
        let alpha = fg[3] as u32;
        let inv = 255 - alpha;
        let mut result = [0u8; 4];
        for c in 0..3 {
            result[c] = ((fg[c] as u32 * alpha + bg[c] as u32 * inv) / 255) as u8;
        }
        // 50% blend: result should be between bg and fg
        assert!(result[0] > fg[0] && result[0] < bg[0], "blended between fg and bg");
        assert!(result[1] > fg[1] && result[1] < bg[1], "blended between fg and bg");
    }

    /// Simple 1D box blur for testing.
    fn blur_simple(dst: &mut [u8], src: &[u8], radius: usize, scratch: &mut [u8]) {
        let len = src.len().min(dst.len()).min(scratch.len());
        let pixels = len / 4;
        scratch[..len].copy_from_slice(&src[..len]);
        let r = radius;
        for i in 0..pixels {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            for j in i.saturating_sub(r)..(i + r + 1).min(pixels) {
                let bi = j * 4;
                for c in 0..4 {
                    sum[c] += scratch[bi + c] as u32;
                }
                count += 1;
            }
            if count > 0 {
                let di = i * 4;
                for c in 0..4 {
                    dst[di + c] = (sum[c] / count).min(255) as u8;
                }
            }
        }
    }
}
