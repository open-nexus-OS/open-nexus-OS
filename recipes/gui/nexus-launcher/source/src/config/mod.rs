// src/config/mod.rs
// Thin re-export layer to keep call sites concise.

pub mod settings;
pub mod colors;

pub use settings::*;
pub use colors::*;
