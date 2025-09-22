// src/services/background_service.rs
// Provides a theme-backed "backdrop" and acrylic panel rendering.
// When no live capture is available, we blur+tint the current wallpaper.
// This gives a convincing acrylic look without screen readback.

use libnexus::themes::{THEME, Paint};
use libnexus::themes::effects::make_acrylic_overlay;
use orbimage::Image;
use orbclient::Renderer;
use log::debug;

/// Try to get a backdrop image sized to the screen.
/// Strategy:
/// 1) Theme background "desktop" with exact size
/// 2) Fallback: "login"
/// 3) Fallback: solid fill image (neutral, based on theme color)
pub fn backdrop_for_screen(screen_w: u32, screen_h: u32) -> Option<Image> {
    if let Some(img) = THEME.load_background("desktop", Some((screen_w, screen_h))) {
        return Some(img);
    }
    if let Some(img) = THEME.load_background("login", Some((screen_w, screen_h))) {
        return Some(img);
    }
    // Solid fallback
    let neutral = THEME.paint("menu_surface_lg_bg", libnexus::themes::Paint {
        color: orbclient::Color::rgba(0, 0, 0, 64),
        acrylic: None
    }).color;
    let mut data = vec![neutral; (screen_w as usize) * (screen_h as usize)];
    Image::from_data(screen_w, screen_h, data.into()).ok()
}

/// Render an acrylic-looking panel into `win` at (x,y,w,h), using `paint`'s acrylic if present.
/// Fallback: plain color fill.
pub fn render_acrylic_panel<R: Renderer>(
    win: &mut R,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    paint: Paint,
) {
    if let Some(ac) = paint.acrylic {
        let (sw, sh) = (win.width(), win.height());
        if let Some(bg) = backdrop_for_screen(sw, sh) {
            let patch = make_acrylic_overlay(&bg, (x, y, w, h), ac);
            // draw the blurred+tinted patch
            patch.draw(win, x, y);
            // optional veil on top for readability (respect paint alpha)
            let c = paint.color;
            if c.a() > 0 {
                win.rect(x, y, w, h, c);
            }
            return;
        }
    }
    // No acrylic configured or no backdrop → plain color
    win.rect(x, y, w, h, paint.color);
}
