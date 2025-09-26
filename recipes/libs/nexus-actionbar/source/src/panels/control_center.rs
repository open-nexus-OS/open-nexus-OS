//! Control Center panel: button grid (4 per row target).
//! Contains two primary toggles (UI Mode, Theme Mode).
//! Slides in from the RIGHT. Respects a bottom gap in Desktop mode.

use orbclient::{Color, Renderer};
use orbfont::Font;
use libnexus::themes::{THEME};
use libnexus::themes::IconVariant;
//use libnexus::themes::effects::make_acrylic_overlay;
use libnexus::ui::layout::conversion::dp_to_px;
use crate::ui::state::ActionBarState;
use crate::ui::paint::fill_rect_with_paint;
use crate::config::{UIMode, ThemeMode};
use crate::config::{
    control_center_group_bg_paint,
    control_center_item_bg_active_paint,
    control_center_item_bg_muted_paint,
};

#[allow(dead_code)]
fn point_in(p: (i32, i32), r: (i32, i32, i32, i32)) -> bool {
    let (x, y) = p; let (rx, ry, rw, rh) = r;
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}
#[inline]
fn try_icon(id_candidates: &[&str], size: (u32, u32)) -> Option<orbimage::Image> {
    for id in id_candidates {
        if let Some(img) = THEME.load_icon_sized(id, IconVariant::Auto, Some(size)) {
            return Some(img);
        }
    }
    None
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

    // --- Compact controls: fixed 35dp circles + small labels ---
    let pad = 12i32;
    let gap = 10i32;
    let cols = 4i32;
    let circle_px = dp_to_px(35, dpi) as i32;
    let label_h  = 14i32;
    let cell_w   = circle_px + 12; // circle + inner padding
    let cell_h   = circle_px + 6 + label_h;

    // Lazy font load (best-effort). Keep it simple and avoid panics if font lookup fails.
    static mut FONT: Option<Font> = None;
    let font = unsafe {
        if FONT.is_none() {
            // orbfont API: find(typeface, family, style) -> Result<Font, String>
            FONT = Font::find(None, None, None).ok();
        }
        FONT.as_ref()
    };

    // We place two primary toggles in the first row:
    // [0] Mode (Desktop/Mobile) — icon only (no label)
    // [1] Theme (Light/Dark)    — icon + label that switches
    // The rest of the grid can be filled with future controls.

    // Helper to draw a circle-ish button with optional label
    let mut draw_circle_button = |col: i32, row: i32, icon_ids: &[&str], active: bool, label: Option<&str>| {        let cx = x + pad + col * (cell_w + gap);
        let cy = y + pad + row * (cell_h + gap);

        // Circle area
        let circle_d = circle_px;
        let rrect = (cx, cy, circle_d as u32, circle_d as u32);

        // Background paint: active vs muted (no icon swap)
        let bg_active = control_center_item_bg_active_paint().color;
        let bg_muted  = control_center_item_bg_muted_paint().color;
        let bg = if active { bg_active } else { bg_muted };

        fill_round_rect(win, rrect.0, rrect.1, rrect.2, rrect.3, (circle_d/2) as i32, bg);

        // Icon (themed, cached by THEME)
        let icon_px = (circle_d as f32 * 0.64).round() as u32;
        if let Some(img) = try_icon(icon_ids, (icon_px, icon_px)) {
            let ix = rrect.0 + (circle_d - img.width() as i32) / 2;
            let iy = rrect.1 + (circle_d - img.height() as i32) / 2;
            img.draw(win, ix, iy);
        }

        // Optional label under the circle
        if let (Some(text), Some(font)) = (label, font) {
            let size = (12.0 * dpi).max(10.0) as f32; // orbfont expects f32
            let color = if active {
                Color::rgba(255,255,255,230)
            } else {
                control_center_item_bg_muted_paint().color
            };
            let rendered = font.render(text, size);
            let tx = cx + (cell_w - rendered.width() as i32) / 2;
            let ty = cy + circle_d + 4;
            rendered.draw(win, tx, ty, color);
        }

        (cx, cy, cell_w, cell_h)
    };

    // MODE button — “Mobile” highlighted when in Mobile
    let is_mobile = matches!(state.ui_mode, UIMode::Mobile);
    let mode_ids = &[
        "controlcenter.mode",
        "controlcenter.button",     // fallback
        "notifications.button",     // last resort
    ];
    let mode_label = if is_mobile { "Mobile" } else { "Desktop" };
    let mode_rect = draw_circle_button(0, 0, mode_ids, is_mobile, Some(mode_label));
    state.cc_hit_mode = Some(mode_rect);

    // THEME button — single icon, background indicates state (no light/dark assets)
    let theme_active = matches!(state.theme_mode, ThemeMode::Dark);
    let theme_ids = &[
        "controlcenter.button",     // primary
        "notifications.button",     // fallback
    ];
    let theme_rect = draw_circle_button(1, 0, theme_ids, theme_active, Some("Theme"));
    state.cc_hit_theme = Some(theme_rect);
}
