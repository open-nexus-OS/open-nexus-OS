// src/core/mod.rs
// Core facilities for window management and the main event loop.

pub mod window_manager;
pub mod event_loop;

pub use window_manager::{WindowManager, WindowZOrder};
pub use event_loop::{EventLoop, UiComponent};
