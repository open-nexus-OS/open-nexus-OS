//! Control Center panel: button grid (4 per row target).
//! Contains two primary toggles (UI Mode, Theme Mode).
//! Slides in from the RIGHT. Respects a bottom gap in Desktop mode.

use orbclient::{Color, Renderer};
use libnexus::themes::{THEME};
use libnexus::themes::IconVariant;
use libnexus::themes::effects::make_acrylic_overlay;
use libnexus::ui::layout::conversion::dp_to_px;
use crate::ui::state::ActionBarState;
use crate::ui::paint::fill_rect_with_paint;
use crate::config::{UIMode, ThemeMode};
use crate::config::{
    control_center_group_bg_paint,
    control_center_item_bg_active_paint,
    control_center_item_fg_muted_paint,
};

#[allow(dead_code)]
fn point_in(p: (i32, i32), r: (i32, i32, i32, i32)) -> bool {
    let (x, y) = p; let (rx, ry, rw, rh) = r;
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

/// Very cheap rounded rect filler for buttons.
fn fill_round_rect<R: Renderer>(win: &mut R, x: i32, y: i32, w: u32, h: u32, r: i32, color: Color) {
    let w_i = w as i32;
    let h_i = h as i32;
    if r <= 0 || w < (2 * r as u32) || h < (2 * r as u32) {
        win.rect(x, y, w, h, color);
        return;
    }
    for yi in 0..h_i {
        let dy = if yi < r { r - 1 - yi } else if yi >= h_i - r { yi - (h_i - r) } else { -1 };
        let (sx, ex) = if dy >= 0 {
            let dx = ((r * r - dy * dy) as f32).sqrt().floor() as i32;
            (x + r - dx, x + w_i - r + dx)
        } else {
            (x, x + w_i)
        };
        let line_w = (ex - sx).max(0) as u32;
        if line_w > 0 { win.rect(sx, y + yi, line_w, 1, color); }
    }
}

pub fn render<R: Renderer>(state: &mut ActionBarState, win: &mut R, screen_w: u32, screen_h: u32) {
    let dpi = state.dpi.max(1.0);

    let w_px = dp_to_px(state.cfg.control_center_width_dp, dpi)
        .min((screen_w as f32 * 0.48) as u32)
        .max(260);

    // Slide progress 0..1
    let t = state.tl_control.value();
    let x = (screen_w as i32 - (t * w_px as f32) as i32).max((screen_w as i32) - (w_px as i32));

    // Below bar, above bottom gap (Desktop) or full to bottom (Mobile)
    let y = state.bar_h_px as i32;
    let h = screen_h.saturating_sub(state.bar_h_px + state.bottom_gap_px());

    // Panel background (acrylic-aware)
    let fallback = libnexus::themes::Paint { color: state.panel_bg, acrylic: None };
    let paint = THEME.paint("panel.controlcenter.bg", fallback);
    fill_rect_with_paint(win, x, y, w_px, h, paint);

    // --- Button grid (4 columns target) ---
    let pad = 16i32;
    let gap = 12i32;
    let cols = 4i32;
    let cell_w = ((w_px as i32 - pad*2 - gap*(cols - 1)) / cols).max(56);
    let cell_h = cell_w + 18; // round button + label area below

    // We place two primary toggles in the first row:
    // [0] Mode (Desktop/Mobile) — icon only (no label)
    // [1] Theme (Light/Dark)    — icon + label that switches
    // The rest of the grid can be filled with future controls.

    // Helper to draw a circle-ish button with optional label
    let mut draw_circle_button = |col: i32, row: i32, icon_id: &str, active: bool, label: Option<&str>| {
        let cx = x + pad + col * (cell_w + gap);
        let cy = y + pad + row * (cell_h + gap);

        // Circle area
        let circle_d = cell_w.min(cell_h - 18);
        let rrect = (cx, cy, circle_d as u32, circle_d as u32);

        let bg_active = control_center_item_bg_active_paint().color;
        let bg_group  = control_center_group_bg_paint().color;
        let bg = if active { bg_active } else { bg_group };

        fill_round_rect(win, rrect.0, rrect.1, rrect.2, rrect.3, (circle_d/2) as i32, bg);

        // Icon (themed, cached by THEME)
        let icon_px = (circle_d as f32 * 0.64).round() as u32;
        if let Some(img) = THEME.load_icon_sized(icon_id, IconVariant::Auto, Some((icon_px, icon_px))) {
            let ix = rrect.0 + (circle_d - img.width() as i32) / 2;
            let iy = rrect.1 + (circle_d - img.height() as i32) / 2;
            img.draw(win, ix, iy);
        }

        // Optional label under the circle
        if let Some(text) = label {
            // Keep labels simple for now (we do not yet bring in orbfont here).
            // A small pill indicates label area; you can replace with real text rendering later.
            let lh = 14;
            let pill_w = (text.len() as i32 * 6).clamp(24, cell_w - 8);
            let pill_x = cx + (cell_w - pill_w) / 2;
            let pill_y = cy + circle_d + 4;
            let mut col = Color::rgba(255,255,255,38);
            if !active {
                col = control_center_item_fg_muted_paint().color;
            }
            fill_round_rect(win, pill_x, pill_y, pill_w as u32, lh as u32, 6, col);
        }

        (cx, cy, cell_w, cell_h)
    };

    // MODE button (icon only). We treat "Mobile active" as highlighted.
    let is_mobile = matches!(state.ui_mode, UIMode::Mobile);
    let mode_icon = "controlcenter.mode"; // same icon, style indicates state
    let mode_rect = draw_circle_button(0, 0, mode_icon, is_mobile, None);
    state.cc_hit_mode = Some(mode_rect);

    // THEME button (icon + label switches)
    let (theme_icon, theme_label, theme_active) = match state.theme_mode {
        ThemeMode::Light => ("controlcenter.light", "Light", true),
        ThemeMode::Dark  => ("controlcenter.dark",  "Dark",  true),
    };
    let theme_rect = draw_circle_button(1, 0, theme_icon, theme_active, Some(theme_label));
    state.cc_hit_theme = Some(theme_rect);
}
