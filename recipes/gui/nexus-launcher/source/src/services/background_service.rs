// src/services/background_service.rs
// Theme-backed backdrop + acrylic panel rendering with proper rounded corners.
//
// If live screen capture is not available, we blur+tint the current wallpaper
// through libnexus. This file provides a single entry point you can call from
// menus/panels:
//
//   render_acrylic_panel(&mut window, x, y, w, h, paint, radius_px);
//
// It will:
//  - build an acrylic-looking patch from the themed backdrop (blur+tint+noise)
//  - draw it clipped to a rounded-rectangle with the given radius
//  - overlay the paint.color to respect opacity/tint
//
// When acrylic is not configured or no backdrop is available, it falls back
// to a rounded rectangle filled with paint.color.

use libnexus::themes::{Paint, THEME};
use libnexus::themes::effects::make_acrylic_overlay;
use orbclient::{Color, Renderer};
use orbimage::Image;

/// Try to get a backdrop image sized to the screen.
/// 1) theme background "desktop" (preferred)
/// 2) fallback: "login"
/// 3) fallback: solid neutral image from theme
pub fn backdrop_for_screen(screen_w: u32, screen_h: u32) -> Option<Image> {
    if let Some(img) = THEME.load_background("desktop", Some((screen_w, screen_h))) {
        return Some(img);
    }
    if let Some(img) = THEME.load_background("login", Some((screen_w, screen_h))) {
        return Some(img);
    }
    // Solid fallback
    let neutral = THEME
        .paint(
            "menu_surface_lg_bg",
            Paint {
                color: Color::rgba(0, 0, 0, 64),
                acrylic: None,
            },
        )
        .color;

    let data = vec![neutral; (screen_w as usize) * (screen_h as usize)];
    Image::from_data(screen_w, screen_h, data.into()).ok()
}

/// Fill a rounded rectangle by drawing horizontal scanlines.
/// This avoids needing a stencil/mask and works on any Renderer.
fn fill_round_rect<R: Renderer>(win: &mut R, x: i32, y: i32, w: u32, h: u32, r: i32, color: Color) {
    let w_i = w as i32;
    let h_i = h as i32;
    if r <= 0 || w < (2 * r as u32) || h < (2 * r as u32) {
        win.rect(x, y, w, h, color);
        return;
    }
    for yi in 0..h_i {
        let dy = if yi < r {
            r - 1 - yi
        } else if yi >= h_i - r {
            yi - (h_i - r)
        } else {
            -1
        };

        let (sx, ex) = if dy >= 0 {
            let dx = ((r * r - dy * dy) as f32).sqrt().floor() as i32;
            (x + r - dx, x + w_i - r + dx)
        } else {
            (x, x + w_i)
        };

        let line_w = (ex - sx).max(0) as u32;
        if line_w > 0 {
            win.rect(sx, y + yi, line_w, 1, color);
        }
    }
}

/// Blit an image into the window, clipped to a rounded-rect by emitting
/// 1-pixel tall sub-images per scanline. This is efficient enough for
/// small/medium panels and keeps edges clean.
fn blit_image_rounded<R: Renderer>(win: &mut R, x: i32, y: i32, patch: &Image, r: i32) {
    let w = patch.width();
    let h = patch.height();
    let w_i = w as i32;
    let h_i = h as i32;

    if r <= 0 || w < (2 * r as u32) || h < (2 * r as u32) {
        patch.draw(win, x, y);
        return;
    }

    let src = patch.data(); // &[Color]
    for yi in 0..h_i {
        let dy = if yi < r {
            r - 1 - yi
        } else if yi >= h_i - r {
            yi - (h_i - r)
        } else {
            -1
        };

        let (sx, ex) = if dy >= 0 {
            let dx = ((r * r - dy * dy) as f32).sqrt().floor() as i32;
            (r - dx, w_i - r + dx)
        } else {
            (0, w_i)
        };

        let line_w = (ex - sx).max(0) as u32;
        if line_w == 0 {
            continue;
        }

        // Build a 1-pixel tall subimage for this scanline slice
        let row = yi as u32;
        let sx_u = sx as u32;
        let ex_u = ex as u32;

        let start = (row * w + sx_u) as usize;
        let end = (row * w + ex_u) as usize;

        // SAFETY: we copy out the slice to a new Vec<Color> the Image will own.
        let line: Vec<Color> = src[start..end].to_vec();
        if let Ok(line_img) = Image::from_data(line_w, 1, line.into()) {
            line_img.draw(win, x + sx, y + yi);
        }
    }
}

/// Render an acrylic panel with rounded corners.
/// If acrylic (blur/tint) is available, it draws the blurred patch clipped to the
/// rounded shape, then overlays paint.color using the same rounded fill.
/// Otherwise it falls back to a rounded fill with paint.color only.
pub fn render_acrylic_panel<R: Renderer>(
    win: &mut R,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    paint: Paint,
    radius: i32,
) {
    let radius = radius.max(0);

    if let Some(ac) = paint.acrylic {
        let (sw, sh) = (win.width(), win.height());
        if let Some(bg) = backdrop_for_screen(sw, sh) {
            // Build acrylic patch from the themed backdrop
            let patch = make_acrylic_overlay(&bg, (x, y, w, h), ac);

            // Draw the blurred+tinted patch clipped to rounded rect
            blit_image_rounded(win, x, y, &patch, radius);

            // Overlay the configured color (same rounded clip), preserving alpha
            let c = paint.color;
            if c.a() > 0 {
                fill_round_rect(win, x, y, w, h, radius, c);
            }
            return;
        }
    }

    // No acrylic or no backdrop → rounded color fill only
    fill_round_rect(win, x, y, w, h, radius, paint.color);
}
