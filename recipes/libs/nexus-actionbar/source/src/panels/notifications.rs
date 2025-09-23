//! Notifications panel: grouped cards per app (skeleton).
//! Slides in from the LEFT: x = -w .. 0 based on timeline progress.

use orbclient::{Color, Renderer};
use libnexus::themes::THEME;
use libnexus::ui::layout::conversion::dp_to_px;
use crate::ui::state::ActionBarState;
use crate::ui::paint::fill_rect_with_paint;

pub fn render<R: Renderer>(state: &mut ActionBarState, win: &mut R, screen_w: u32, screen_h: u32) {
    let dpi = state.dpi.max(1.0);

    let w_px = dp_to_px(state.cfg.notifications_width_dp, dpi)
        .min((screen_w as f32 * 0.42) as u32)
        .max(240);

    // Slide progress 0..1
    let t = state.tl_notifications.value();
    let x = (-(w_px as i32) + (t * w_px as f32) as i32).min(0);

    // Below bar, above bottom gap (Desktop) or full to bottom (Mobile)
    let y = state.bar_h_px as i32;
    let h = screen_h.saturating_sub(state.bar_h_px + state.bottom_gap_px());

    // Panel paint (may include acrylic)
    let fallback = libnexus::themes::Paint { color: state.panel_bg, acrylic: None };
    let paint = THEME.paint("panel.notifications.bg", fallback);

    // Acrylic-aware fill
    fill_rect_with_paint(win, x, y, w_px, h, paint);

    // Placeholder grouped rows
    let pad = 12i32;
    let card_h = 64i32;
    let gap = 10i32;
    let mut cy = y + pad;

    for _ in 0..3 {
        win.rect(x + pad, cy, (w_px as i32 - pad*2) as u32, 18, Color::rgba(255,255,255,38));
        cy += 18 + 6;

        for _ in 0..2 {
            win.rect(x + pad, cy, (w_px as i32 - pad*2) as u32, card_h as u32, Color::rgba(0,0,0,26));
            cy += card_h + gap;
        }

        cy += gap;
        if (cy as u32) >= (y as u32 + h).saturating_sub(20) { break; }
    }
}
