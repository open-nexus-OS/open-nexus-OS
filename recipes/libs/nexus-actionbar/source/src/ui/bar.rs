//! Render the top action bar and its two toggle buttons.

use orbclient::Renderer;
use libnexus::themes::THEME;
use super::state::ActionBarState;
use super::paint::fill_rect_with_paint;

/// Draw the bar background + buttons.
pub fn render<R: Renderer>(state: &mut ActionBarState, win: &mut R, y: i32, w: u32) {
    // Resolve bar paint (can include acrylic)
    let fallback = libnexus::themes::Paint { color: state.bar_bg, acrylic: None };
    let paint = THEME.paint("actionbar_bg", fallback);

    // Layout (updates button rects)
    state.layout_bar(state.dpi.max(1.0), y, w);

    // Acrylic-aware fill
    fill_rect_with_paint(win, 0, y, w, state.bar_h_px, paint);

    // Draw buttons
    let icon_px = (state.bar_h_px as f32 * 0.66).round() as u32;
    state.btn_notifications.draw(win, state.hover_veil, icon_px);
    state.btn_control.draw(win, state.hover_veil, icon_px);
}
