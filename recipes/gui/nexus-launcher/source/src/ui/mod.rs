// ui/mod.rs - UI components and layout

pub mod components;
pub mod layout;
pub mod icons;
pub mod chooser_handler;
// pub mod bar_handler; // Temporarily disabled - needs to be implemented

// Re-export main functions for easy access
pub use chooser_handler::chooser_main;
// pub use bar_handler::bar_main; // Temporarily disabled
