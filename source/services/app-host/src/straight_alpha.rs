// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pixel format app-host hands the compositor — BGRA, straight alpha (RFC-0109,
//! "The surface"). The painter blends in premultiplied form (`nexus_scene_raster::RowCanvas`), so
//! every row that leaves app-host — a base row, a band row, a motion layer's row — is divided out
//! once, where its alpha is partial; the compositor premultiplies it again (`layer.wgsl`,
//! `straight`). A row handed over premultiplied is multiplied by its alpha twice: the light
//! glass tint `#ffffff80` reached the screen as a quarter-white, and the light dock went grey.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the unit tests below (the painter's pixel, the round trip, the reciprocal, the
//! untouched ends, a base that never outshines its alpha).

/// `255 · 2¹⁶ / a`, rounded, for every alpha: a channel is divided out by one multiply — the
/// board's divide costs tens of cycles, and a glass tint makes every pixel of a bar partial.
const RECIP: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut a = 1;
    while a < 256 {
        t[a] = ((255u32 << 16) + (a as u32) / 2) / a as u32;
        a += 1;
    }
    t
};

/// Turns a span of premultiplied BGRA pixels into straight alpha, in place. An opaque or a
/// transparent pixel is its own straight form and stays as it is; four of them in a row — a
/// pane, the desktop's clear ground — are passed by one test.
pub(crate) fn unpremultiply(span: &mut [u8]) {
    let mut quads = span.chunks_exact_mut(16);
    for q in &mut quads {
        let (a0, a1, a2, a3) = (q[3], q[7], q[11], q[15]);
        if a0 & a1 & a2 & a3 == 255 || a0 | a1 | a2 | a3 == 0 {
            continue;
        }
        for px in q.chunks_exact_mut(4) {
            divide(px);
        }
    }
    for px in quads.into_remainder().chunks_exact_mut(4) {
        divide(px);
    }
}

/// `c` at alpha `a` as the painter's premultiplied BGRA pixel — what a row is cleared to before
/// it is painted (the page base, a band's ground). A colour never outshines its alpha: a clear
/// transparent base left the light `glassWindow` bytes standing under alpha 0, and every
/// translucent pixel blended over them took that white in (the light desktop's 56 % black icons
/// came out near-white, board cycle 2026-11-10).
pub(crate) fn premultiplied(c: nexus_layout_types::Rgba8, a: u8) -> [u8; 4] {
    let pm = |v: u8| ((u32::from(v) * u32::from(a) + 127) / 255) as u8;
    [pm(c.b), pm(c.g), pm(c.r), a]
}

/// One pixel divided out by its alpha (`c · 255 / a`, rounded, by [`RECIP`]).
#[inline]
fn divide(px: &mut [u8]) {
    let a = px[3];
    if a == 0 || a == 255 {
        return;
    }
    let r = RECIP[usize::from(a)];
    for c in &mut px[..3] {
        *c = ((u32::from(*c) * r + (1 << 15)) >> 16).min(255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::{premultiplied, unpremultiply};
    use nexus_layout_types::Rgba8;
    use nexus_scene_raster::RowCanvas;

    /// The compositor's straight pipeline: the texel's colour times its alpha.
    fn premultiply(px: [u8; 4]) -> [u8; 4] {
        let a = u32::from(px[3]);
        let m = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
        [m(px[0]), m(px[1]), m(px[2]), px[3]]
    }

    #[test]
    fn the_light_glass_tint_reaches_the_compositor_as_white_at_half_alpha() {
        // The dock's light tint, painted onto a transparent row the way a glass root is.
        let mut row = [0u8; 4 * 4];
        let mut canvas = RowCanvas::new(&mut row, 0, 4);
        canvas.blend(1, Rgba8 { r: 255, g: 255, b: 255, a: 128 });
        assert_eq!(row[4..8], [128, 128, 128, 128], "the painter blends premultiplied");
        unpremultiply(&mut row);
        assert_eq!(row[4..8], [255, 255, 255, 128], "the compositor reads white at half alpha");
        // Multiplied once more by the compositor, it is the painter's pixel again — not the
        // quarter-white the unconverted row became.
        assert_eq!(premultiply([255, 255, 255, 128]), [128, 128, 128, 128]);
    }

    #[test]
    fn a_converted_pixel_premultiplied_again_is_the_painters_pixel() {
        for a in 1..=254u8 {
            for c in 0..=a {
                let mut px = [c, c / 2, 0, a];
                let painted = px;
                unpremultiply(&mut px);
                let back = premultiply(px);
                for k in 0..3 {
                    let d = i32::from(back[k]) - i32::from(painted[k]);
                    assert!(d.abs() <= 1, "a={a} c={c}: {painted:?} → {px:?} → {back:?}");
                }
            }
        }
    }

    /// The reciprocal is the division: within one step of `c · 255 / a` (rounded) for every
    /// premultiplied pixel (`c ≤ a`), and a mixed run of four — the quad test passes it on —
    /// converts like four pixels one by one.
    #[test]
    fn the_reciprocal_is_the_division_and_a_mixed_quad_converts_pixel_by_pixel() {
        for a in 1..=254u32 {
            for c in 0..=a {
                let mut px = [c as u8, 0, 0, a as u8];
                unpremultiply(&mut px);
                let exact = (c * 255 + a / 2) / a;
                assert!(u32::from(px[0]).abs_diff(exact) <= 1, "a={a} c={c}: {} vs {exact}", px[0]);
            }
        }
        let mut quad = [64, 64, 64, 128, 1, 2, 3, 255, 0, 0, 0, 0, 10, 20, 30, 40];
        let mut one_by_one = quad;
        unpremultiply(&mut quad);
        for px in one_by_one.chunks_exact_mut(4) {
            unpremultiply(px);
        }
        assert_eq!(quad, one_by_one);
        assert_eq!(quad[..4], [128, 128, 128, 128], "half-alpha mid grey is mid grey");
    }

    /// The light desktop's base is `glassWindow` at alpha 0. Cleared to it premultiplied, a
    /// 56 % black icon blended over it leaves black at 56 %; cleared to the bare colour bytes,
    /// the same icon left near-white — what the board showed in light mode.
    #[test]
    fn test_reject_a_base_whose_colour_outshines_its_alpha() {
        let glass_window = Rgba8 { r: 252, g: 252, b: 254, a: 209 };
        let icon = Rgba8 { r: 0, g: 0, b: 0, a: 143 };
        let paint = |base: [u8; 4]| {
            let mut row = [0u8; 4];
            row.copy_from_slice(&base);
            RowCanvas::new(&mut row, 0, 1).blend(0, icon);
            unpremultiply(&mut row);
            row
        };
        assert_eq!(
            premultiplied(glass_window, 0),
            [0, 0, 0, 0],
            "transparent is transparent black"
        );
        assert_eq!(paint(premultiplied(glass_window, 0)), [0, 0, 0, 143]);
        let bled = paint([254, 252, 252, 0]);
        assert!(bled[0] > 150, "the bare bytes bleed white into the icon: {bled:?}");
        // An opaque colour is its own premultiplication; a translucent one is scaled.
        assert_eq!(premultiplied(glass_window, 255), [254, 252, 252, 255]);
        assert_eq!(premultiplied(glass_window, 128), [127, 126, 126, 128]);
    }

    #[test]
    fn opaque_and_transparent_pixels_stay_as_they_are() {
        let mut span = [10, 20, 30, 255, 0, 0, 0, 0, 7, 8, 9, 0];
        let before = span;
        unpremultiply(&mut span);
        assert_eq!(span, before);
    }
}
