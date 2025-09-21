//! Control Center panel: button grid (4 per row target).
//! For now: only two toggles (Mobile Mode, Dark Mode) but the grid is prepared.
//! Slides in from the RIGHT: x = screen_w .. screen_w - w based on timeline progress.

use orbclient::{Color, Renderer};
use libnexus::themes::THEME;
use libnexus::ui::layout::conversion::dp_to_px;
use crate::ui::state::ActionBarState;

pub fn render<R: Renderer>(state: &mut ActionBarState, win: &mut R, screen_w: u32, screen_h: u32) {
    let dpi = state.dpi.max(1.0);

    let w_px = dp_to_px(state.cfg.control_center_width_dp, dpi)
        .min((screen_w as f32 * 0.48) as u32)
        .max(260);

    // Slide progress 0..1
    let t = state.tl_control.value();
    let x = (screen_w as i32 - (t * w_px as f32) as i32).max((screen_w as i32) - (w_px as i32));

    let y = state.bar_h_px as i32;
    let h = (screen_h - state.bar_h_px).max(0);

    let paint = THEME.paint("panel.controlcenter.bg", libnexus::themes::Paint {
        color: state.panel_bg, acrylic: None
    });
    win.rect(x, y, w_px, h, paint.color);

    // 4-column grid, but we only show two toggles initially.
    let pad = 16i32;
    let gap = 12i32;
    let cols = 4i32;
    let cell_w = ((w_px as i32 - pad*2 - gap*(cols - 1)) / cols).max(56);
    let cell_h = cell_w; // square buttons

    // Example two buttons: "Mobile Mode" and "Dark Mode"
    let mut draw_cell = |col: i32, row: i32, _label: &str, active: bool| {
        let cx = x + pad + col * (cell_w + gap);
        let cy = y + pad + row * (cell_h + gap);
        let bg = if active { Color::rgba(0, 180, 90, 120) } else { Color::rgba(255,255,255,34) };
        win.rect(cx, cy, cell_w as u32, cell_h as u32, bg);

        // Placeholder label bar
        let lh = 14;
        win.rect(cx, cy + cell_h - lh - 6, cell_w as u32, lh as u32, Color::rgba(0,0,0,50));
    };

    draw_cell(0, 0, "Mobile", false);
    draw_cell(1, 0, "Dark",   false);
}
