// themes/effects.rs acrylic overlay helper
use orbclient::{Color, Renderer}; 
use orbimage::Image;

use super::colors::Acrylic;
use super::svg_icons::scale_nearest;

/// Create an acrylic-looking overlay for a given screen region:
/// 1) crop the source region
/// 2) downscale then upscale (fast blur approximation)
/// 3) apply tint and optional noise
pub fn make_acrylic_overlay(src: &Image, area: (i32,i32,u32,u32), a: Acrylic) -> Image {
    let (x, y, w, h) = area;

    // Crop safely inside bounds
    let crop = crop_image(src, x, y, w, h);

    // Cheap blur: downscale then upscale back to original size
    let ds = a.downscale.max(2) as u32;
    let small = scale_nearest(&crop, (w / ds.max(1)).max(1), (h / ds.max(1)).max(1));
    let mut out = scale_nearest(&small, w, h);

    // Tint blend over the blurred buffer
    if a.tint.a() > 0 {
        blend_color(&mut out, a.tint);
    }

    // Optional noise to avoid flatness / banding
    if a.noise_alpha > 0 {
        add_noise(&mut out, a.noise_alpha);
    }
    out
}

fn crop_image(src: &Image, x: i32, y: i32, w: u32, h: u32) -> Image {
    let sw = src.width();
    let sh = src.height();
    let sx = x.max(0) as u32;
    let sy = y.max(0) as u32;

    let mut buf = Vec::with_capacity((w*h) as usize);
    for yy in 0..h {
        let py = (sy + yy).min(sh.saturating_sub(1));
        for xx in 0..w {
            let px = (sx + xx).min(sw.saturating_sub(1));
            buf.push(src.data()[(py * sw + px) as usize]);
        }
    }
    Image::from_data(w, h, buf.into()).unwrap()
}

fn blend_color(img: &mut Image, tint: Color) {
    let a = tint.a() as u32;
    if a == 0 { return; }
    let inv = 255 - a;

    // Safety: orbimage::Image exposes &mut [Color] via data_mut()
    let data = img.data_mut();
    for c in data.iter_mut() {
        *c = Color::rgba(
            ((c.r() as u32 * inv + tint.r() as u32 * a) / 255) as u8,
            ((c.g() as u32 * inv + tint.g() as u32 * a) / 255) as u8,
            ((c.b() as u32 * inv + tint.b() as u32 * a) / 255) as u8,
            c.a(),
        );
    }
}

fn add_noise(img: &mut Image, alpha: u8) {
    // Very small, fast PRNG; good enough for visual noise.
    let mut seed: u32 = 0x1234ABCD;
    #[inline] fn rnd(s: &mut u32) -> u8 {
        *s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (*s >> 24) as u8
    }

    let mix = alpha as u32;
    let inv = 255 - mix;

    let data = img.data_mut();
    for c in data.iter_mut() {
        let n = rnd(&mut seed) as u32;
        *c = Color::rgba(
            ((c.r() as u32 * inv + n * mix) / 255) as u8,
            ((c.g() as u32 * inv + n * mix) / 255) as u8,
            ((c.b() as u32 * inv + n * mix) / 255) as u8,
            c.a(),
        );
    }
}
