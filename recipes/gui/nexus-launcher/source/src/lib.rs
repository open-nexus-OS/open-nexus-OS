// src/lib.rs - Library root with proper module structure

// New modular structure
pub mod config;
pub mod ui;
pub mod services;
pub mod utils;
pub mod types;
pub mod core;

// Existing modules (keep as-is for now)
pub mod modes;
pub mod package;

// Re-export commonly used items for compatibility
pub use config::settings::{Mode, set_mode, mode};
pub use config::colors::{bar_paint, bar_highlight_paint, bar_activity_marker_paint};
pub use config::colors::{text_paint, text_highlight_paint, text_inverse_fg, text_fg};
pub use config::colors::load_crisp_font;
pub use config::settings::{BAR_HEIGHT, ICON_SCALE, ICON_SMALL_SCALE};

pub use ui::layout::{SearchState, GridLayout, compute_grid, grid_iter_and_hit};
pub use ui::components::draw_app_cell;
pub use package::{IconSource, Package};
pub use services::icon_service::CommonIcons;
pub use utils::dpi_helper::get_dpi_scale;

// Re-export dpi_scale function for compatibility
pub fn dpi_scale() -> f32 {
    utils::dpi_helper::get_dpi_scale()
}
