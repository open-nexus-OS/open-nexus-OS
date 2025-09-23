//! Public API for the Action Bar as a library crate (no own event loop).

pub mod config;
pub mod ui;
pub mod panels;

use orbclient::{Event as OrbEvent, Renderer};

use libnexus::{Insets, AnimationManager};
use ui::state::{ActionBarState};

/// High-level wrapper the launcher owns.
pub struct ActionBar {
    state: ActionBarState,
}

impl ActionBar {
    pub fn new(cfg: config::Config) -> Self {
        Self { state: ActionBarState::new(cfg) }
    }

    pub fn required_insets(&self, _screen_w: u32, _screen_h: u32, dpi: f32) -> Insets {
        libnexus::ui::layout::required_insets(self.state.cfg.height_dp, dpi)
    }

    pub fn update(&mut self, dt_ms: u32) { self.state.update(dt_ms); }

    pub fn is_animating(&self) -> bool { self.state.is_animating() }

    pub fn handle_event(&mut self, ev: &OrbEvent) -> Option<ActionBarMsg> {
        self.state.handle_event(ev)
    }

    pub fn render_bar<R: Renderer>(&mut self, win: &mut R, y: i32, w: u32) {
        ui::bar::render(&mut self.state, win, y, w);
    }

    pub fn render_panels<R: Renderer>(&mut self, win: &mut R, screen_w: u32, screen_h: u32) {
        panels::render(&mut self.state, win, screen_w, screen_h);
    }

    pub fn any_panel_open(&self) -> bool { self.state.any_panel_open() }

    pub fn dismiss_panels(&mut self) { self.state.dismiss_panels(); }

    pub fn set_animation_manager(&mut self, manager: &mut AnimationManager) {
        self.state.set_animation_manager(manager);
    }
}

// Re-exports for convenience
pub use config::{Config, UIMode, ThemeMode};
pub use ui::state::ActionBarMsg;
