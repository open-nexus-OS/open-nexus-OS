//! Acrylic-aware rectangle fill for bar/panels.
//! Uses THEME backgrounds + effects::make_acrylic_overlay as a cheap blur.
//! Falls back to a plain color if no background is available.

use orbclient::Renderer;
use libnexus::themes::{THEME, Paint};
use libnexus::themes::effects::make_acrylic_overlay;

/// Fill a rect with the given paint.
/// If `paint.acrylic` is Some, we try to synthesize an acrylic patch from the current theme
/// background (desktop/login). Otherwise, we draw the base color.
pub fn fill_rect_with_paint<R: Renderer>(
    win: &mut R,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    paint: Paint,
) {
    // Snapshot dimensions BEFORE passing &mut win anywhere else.
    let sw = win.width();
    let sh = win.height();

    if let Some(ac) = paint.acrylic {
        if let Some(bg) = THEME.load_background("desktop", Some((sw, sh)))
            .or_else(|| THEME.load_background("login", Some((sw, sh))))
        {
            let patch = make_acrylic_overlay(&bg, (x, y, w, h), ac);
            patch.draw(win, x, y);
            if paint.color.a() > 0 {
                win.rect(x, y, w, h, paint.color);
            }
            return;
        }
    }
    // Fallback (no acrylic set or no background available)
    win.rect(x, y, w, h, paint.color);
}
