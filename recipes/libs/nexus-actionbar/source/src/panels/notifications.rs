//! Notifications panel: grouped cards per app (skeleton).
//! For now: a single acrylic-like background with placeholder rows.
//! Slides in from the LEFT: x = -w .. 0 based on timeline progress.

use orbclient::{Color, Renderer};
use libnexus::themes::THEME;
use libnexus::ui::layout::conversion::dp_to_px;
use crate::ui::state::ActionBarState;

pub fn render<R: Renderer>(state: &mut ActionBarState, win: &mut R, screen_w: u32, screen_h: u32) {
    let dpi = state.dpi.max(1.0);

    let w_px = dp_to_px(state.cfg.notifications_width_dp, dpi)
        .min((screen_w as f32 * 0.42) as u32)
        .max(240);

    // Slide progress 0..1
    let t = state.tl_notifications.value();
    let x = (-(w_px as i32) + (t * w_px as f32) as i32).min(0);

    // Top offset: below the bar
    let y = state.bar_h_px as i32;
    let h = (screen_h - state.bar_h_px).max(0);

    let paint = THEME.paint("panel.notifications.bg", libnexus::themes::Paint {
        color: state.panel_bg, acrylic: None
    });

    // Panel background
    win.rect(x, y, w_px, h, paint.color);

    // Placeholder grouped rows (3 sample cards)
    let pad = 12i32;
    let card_h = 64i32;
    let gap = 10i32;
    let mut cy = y + pad;

    for _ in 0..3 {
        // group header placeholder
        win.rect(x + pad, cy, (w_px as i32 - pad*2) as u32, 18, Color::rgba(255,255,255,38));
        cy += 18 + 6;

        // a few notification items
        for _ in 0..2 {
            win.rect(x + pad, cy, (w_px as i32 - pad*2) as u32, card_h as u32, Color::rgba(0,0,0,26));
            cy += card_h + gap;
        }

        cy += gap;
        if (cy as u32) >= h.saturating_sub(20) { break; }
    }
}
