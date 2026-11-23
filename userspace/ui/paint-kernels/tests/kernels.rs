// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the per-pixel kernels of TASK-0367 P4 (`cover`, `over`, `unpremultiply`) on the host:
//! each reference is the painter's own pixel for every input, and the arithmetic the vector forms
//! do in their 32-bit lanes (`riscv.rs`) is the reference's for every input — so what the
//! assembly can still get wrong is the loop, which the proof's digest checks on every machine
//! (`SELFTEST: paint kernels ok`, the user-mode emulator at several register widths).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: this file.

use nexus_paint_kernels::reference::{self, Over, STRAIGHT};

/// `RowCanvas::blend` of the straight colour `[b, g, r, a]` over one premultiplied pixel — the
/// painter's pixel before the kernels, kept as the witness.
fn painters_pixel(dst: [u8; 4], [b, g, r, a]: [u8; 4]) -> [u8; 4] {
    if a == 0 {
        return dst;
    }
    let (a, inv) = (u32::from(a), 255 - u32::from(a));
    let mix = |d: u8, s: u8| ((u32::from(d) * inv + u32::from(s) * a) / 255) as u8;
    [mix(dst[0], b), mix(dst[1], g), mix(dst[2], r), (a + u32::from(dst[3]) * inv / 255) as u8]
}

/// app-host's straight-alpha pixel before it moved here (`straight_alpha::divide`).
fn app_hosts_pixel(mut px: [u8; 4]) -> [u8; 4] {
    let a = u32::from(px[3]);
    if a == 0 || a == 255 {
        return px;
    }
    let r = ((255u32 << 16) + a / 2) / a;
    for c in &mut px[..3] {
        *c = ((u32::from(*c) * r + (1 << 15)) >> 16).min(255) as u8;
    }
    px
}

#[test]
fn cover_and_over_are_the_painters_pixel_for_every_alpha_destination_and_colour() {
    for a in 0..=255u8 {
        for d in 0..=255u8 {
            let dst = [d, 255 - d, d / 3, d];
            for c in (0..=255u8).step_by(3).chain([254, 255]) {
                let src = [c, 255 - c, c / 2, a];
                let want = painters_pixel(dst, src);
                let mut px = dst;
                reference::cover(&mut px, [src[0], src[1], src[2]], &[a]);
                assert_eq!(px, want, "cover dst={dst:?} src={src:?}");
                // A sprite's pixel is red, green, blue, alpha.
                let mut px = dst;
                reference::over(&mut px, &[src[2], src[1], src[0], a]);
                assert_eq!(px, want, "over dst={dst:?} src={src:?}");
            }
        }
    }
}

#[test]
fn the_cover_and_over_lanes_are_the_reference_for_every_input() {
    // A channel in a 32-bit lane: `((d · (255 − a) + s · a) · 0x8081) >> 23`, products in 32
    // bits as `vmul` keeps them; the alpha channel's `s` is 255.
    let lane = |d: u32, s: u32, a: u32| {
        (d.wrapping_mul(255 - a).wrapping_add(s.wrapping_mul(a))).wrapping_mul(0x8081) >> 23
    };
    for a in 0..=255u8 {
        for s in 0..=255u8 {
            let over = Over::of([s, 0, 0, a]);
            for d in 0..=255u8 {
                let got = lane(u32::from(d), u32::from(s), u32::from(a));
                assert_eq!(got, u32::from(over.channel(d, 0)), "d={d} s={s} a={a}");
                assert!(got <= 255);
            }
        }
        for d in 0..=255u8 {
            let got = lane(u32::from(d), 255, u32::from(a));
            assert_eq!(got, u32::from(Over::of([0, 0, 0, a]).channel(d, 3)), "alpha d={d} a={a}");
        }
    }
}

#[test]
fn unpremultiply_is_app_hosts_pixel_and_its_lane_for_every_channel_and_alpha() {
    for a in 0..=255u8 {
        for c in 0..=255u8 {
            let px = [c, 255 - c, c / 2, a];
            let mut got = px;
            reference::unpremultiply(&mut got);
            assert_eq!(got, app_hosts_pixel(px), "px={px:?}");
            // The vector lane: every pixel multiplied by its entry, the alpha byte kept.
            let lane = |v: u8| {
                (u32::from(v).wrapping_mul(STRAIGHT[usize::from(a)]).wrapping_add(1 << 15) >> 16)
                    .min(255) as u8
            };
            assert_eq!([lane(px[0]), lane(px[1]), lane(px[2]), a], got, "lane px={px:?}");
        }
    }
    // The product never wraps: the largest is a channel of 255 at alpha 1.
    assert!(u64::from(STRAIGHT[1]) * 255 + (1 << 15) < 1 << 32);
}

#[test]
fn a_kernel_paints_whole_pixels_as_many_as_both_rows_hold_and_leaves_the_rest() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed as u8
    };
    let mut base = [0u8; 4 * 80 + 8];
    base.iter_mut().for_each(|b| *b = next());
    let mut second = [0u8; 4 * 80 + 8];
    second.iter_mut().for_each(|b| *b = next());
    for at in 0..4 {
        for len in 0..=4 * 72 + 3 {
            for other in [0, len / 8, len / 4, len / 4 + 3, len, len + 5] {
                let pixels = |n: usize| 4 * (len / 4).min(n);
                // Each kernel, and the bytes it may paint: whole pixels, as many as both hold.
                for (kernel, painted) in
                    [(0, pixels(other)), (1, pixels(other / 4)), (2, pixels(len))]
                {
                    let mut row = base;
                    let span = &mut row[at..at + len];
                    match kernel {
                        0 => nexus_paint_kernels::cover(span, [9, 99, 199], &second[..other]),
                        1 => nexus_paint_kernels::over(span, &second[..other]),
                        _ => nexus_paint_kernels::unpremultiply(span),
                    }
                    assert_eq!(row[..at], base[..at], "kernel={kernel} at={at} len={len}");
                    assert_eq!(
                        row[at + painted..],
                        base[at + painted..],
                        "kernel={kernel} len={len}"
                    );
                }
            }
        }
    }
}

#[test]
fn an_empty_row_or_an_empty_second_row_changes_nothing() {
    let mut row = [7u8; 64];
    nexus_paint_kernels::cover(&mut row, [1, 2, 3], &[]);
    nexus_paint_kernels::over(&mut row, &[255; 3]);
    nexus_paint_kernels::cover(&mut [], [1, 2, 3], &[255; 8]);
    nexus_paint_kernels::over(&mut [], &[255; 8]);
    nexus_paint_kernels::unpremultiply(&mut []);
    assert_eq!(row, [7u8; 64]);
}
