// Simplified DPI scaling system for nexus-launcher
// This module provides a clean, easy-to-debug approach to DPI scaling

use std::sync::atomic::{AtomicIsize, Ordering};

// Global DPI scale factor (100 = 100%, 150 = 150%, etc.)
static DPI_SCALE: AtomicIsize = AtomicIsize::new(100);

/// Simple DPI scale calculation based on screen resolution
/// Much easier to debug and understand than complex estimation
pub fn calculate_dpi_scale(width: u32, height: u32) -> f32 {
    let total_pixels = width * height;

    if total_pixels < 1_000_000 {
        0.75  // Small screens (e.g., 1024x768)
    } else if total_pixels < 2_000_000 {
        1.0   // Medium screens (e.g., 1366x768, 1440x900)
    } else if total_pixels < 4_000_000 {
        1.25  // Large screens (e.g., 1920x1080)
    } else if total_pixels < 8_000_000 {
        1.5   // Very large screens (e.g., 2560x1440)
    } else {
        2.0   // Ultra-wide screens (e.g., 3840x2160)
    }
}

/// Set the global DPI scale
pub fn set_dpi_scale(scale: f32) {
    let scale_percent = (scale * 100.0) as isize;
    DPI_SCALE.store(scale_percent, Ordering::Relaxed);
}

/// Get the current DPI scale factor
pub fn get_dpi_scale() -> f32 {
    DPI_SCALE.load(Ordering::Relaxed) as f32 / 100.0
}

/// Calculate icon size with DPI scaling
pub fn icon_size(base_size: f32) -> i32 {
    let dpi_scale = get_dpi_scale();
    (base_size * dpi_scale).round() as i32
}

/// Calculate font size with DPI scaling
pub fn font_size(base_size: f32) -> f32 {
    let dpi_scale = get_dpi_scale();
    base_size * dpi_scale
}

/// Calculate any size with DPI scaling
pub fn scale_size(size: f32) -> f32 {
    let dpi_scale = get_dpi_scale();
    size * dpi_scale
}

/// Initialize DPI scaling based on screen size
pub fn init_dpi_scaling(width: u32, height: u32) {
    let scale = calculate_dpi_scale(width, height);
    set_dpi_scale(scale);
    println!("DPI scaling initialized: {:.2}x ({}x{} screen)", scale, width, height);
}
