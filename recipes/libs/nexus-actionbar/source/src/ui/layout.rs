//! Geometry and layout helpers (dp→px conversion, hit rects, insets).

use super::state::ActionBarState;

#[derive(Copy, Clone, Default, Debug)]
pub struct Insets {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

/// Convert dp (logical) to px using the given dpi scale.
#[inline]
pub fn dp_to_px(dp: u32, dpi: f32) -> u32 {
    ((dp as f32) * dpi).round().max(1.0) as u32
}

/// Compute insets given state + dpi.
/// For now only top inset is used.
pub fn required_insets(state: &ActionBarState, _screen_w: u32, dpi: f32) -> Insets {
    Insets {
        top: dp_to_px(state.cfg.height_dp, dpi),
        ..Default::default()
    }
}

/// Button slots: fixed square areas inside the bar.
/// Left slot at x=0, right slot at bar_width - slot_px.
pub fn button_slot_px(bar_h_px: u32) -> i32 {
    // Slightly larger hit target than the visible icon.
    (bar_h_px as i32).max(32)
}
