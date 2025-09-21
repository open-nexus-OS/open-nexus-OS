//! Layout conversion utilities for DPI scaling and UI calculations.

/// Convert density-independent pixels (dp) to actual pixels based on DPI scale.
pub fn dp_to_px(dp: u32, dpi_scale: f32) -> u32 {
    (dp as f32 * dpi_scale.max(1.0)).round() as u32
}

/// Calculate button slot size in pixels based on bar height.
pub fn button_slot_px(bar_height_px: u32) -> u32 {
    // Button should be 80% of bar height with some padding
    (bar_height_px as f32 * 0.8).round() as u32
}

/// Calculate required insets for the action bar.
pub fn required_insets(bar_height_dp: u32, dpi: f32) -> super::Insets {
    let bar_height = dp_to_px(bar_height_dp, dpi);

    super::Insets::new(0, bar_height)
}

// Note: This module needs to be imported by libnexus/ui/layout/mod.rs
