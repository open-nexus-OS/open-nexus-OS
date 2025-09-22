// src/services/mod.rs
// OS-/I/O-facing helpers and data sources.

pub mod app_catalog;
pub mod package_service;
pub mod process_manager;
pub mod theme;
pub mod background_service;

pub use app_catalog::*;
pub use package_service::*;
pub use process_manager::*;
pub use theme::*;
