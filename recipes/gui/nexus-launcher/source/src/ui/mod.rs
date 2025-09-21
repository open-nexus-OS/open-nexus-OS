// ui/mod.rs - UI components and layout

pub mod components;
pub mod layout;
pub mod icons;
pub mod chooser_handler;
pub mod bar_handler;
pub mod bar_core;

// Re-export main functions for easy access
pub use chooser_handler::chooser_main;
pub use bar_handler::bar_main;
