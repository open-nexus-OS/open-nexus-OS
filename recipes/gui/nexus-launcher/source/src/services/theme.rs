// src/services/theme.rs
// Small convenience wrappers around THEME for launcher use.
// Also provides a fallback acrylic overlay path until we have real screen sampling.

use libnexus::themes::{THEME, Paint, Acrylic};
use orbclient::{Color, Renderer};
use orbimage::Image;

/// Shortcut: fetch a paint by name with a fallback.
pub fn paint(name: &str, fallback: Paint) -> Paint {
    THEME.paint(name, fallback)
}

/// Shortcut: acrylic-only by name.
pub fn acrylic(name: &str) -> Option<Acrylic> {
    THEME.acrylic(name)
}

/// Fallback “acrylic-like” overlay if you cannot sample the screen region yet.
/// This simply fills the rect with the base color and applies the tint
/// (no blur). When you implement real capture, switch to
/// libnexus::themes::effects::make_acrylic_overlay.
pub fn acrylic_overlay_fallback(win: &mut dyn Renderer, x: i32, y: i32, w: u32, h: u32, paint: Paint) {
    // Base fill
    win.rect(x, y, w, h, paint.color);
    // Optional tint if present
    if let Some(a) = paint.acrylic {
        if a.tint.a() > 0 {
            win.rect(x, y, w, h, a.tint);
        }
    }
}

/// When you do have a sampled source image of the area behind a rect,
/// you can build a real acrylic overlay using libnexus’ effect helper:
///
/// let overlay = libnexus::themes::effects::make_acrylic_overlay(
///     &captured_screen, (x, y, w, h), acrylic);
/// overlay.draw(win, x, y);
///
/// For now, callers can use `acrylic_overlay_fallback`.
pub fn make_real_acrylic_overlay_if_you_have_source(
    _src: &Image, _area: (i32, i32, u32, u32), _a: Acrylic
) -> Image {
    // Placeholder to document the intended usage; not called by default.
    Image::default()
}
