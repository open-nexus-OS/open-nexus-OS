//! Public API for the Action Bar as a library crate (no own event loop).

pub mod config;
pub mod ui;
pub mod panels;

use orbclient::{Event, Renderer};

use ui::layout::Insets;
use ui::state::ActionBarState;

/// High-level wrapper the launcher owns.
pub struct ActionBar {
    state: ActionBarState,
}

impl ActionBar {
    /// Create a new Action Bar with the given configuration.
    pub fn new(cfg: config::Config) -> Self {
        Self { state: ActionBarState::new(cfg) }
    }

    /// Insets the window manager should reserve (top bar in px).
    pub fn required_insets(&self, screen_w: u32, _screen_h: u32, dpi: f32) -> Insets {
        ui::layout::required_insets(&self.state, screen_w, dpi)
    }

    /// Advance animations by `dt_ms` milliseconds.
    pub fn update(&mut self, dt_ms: u32) {
        self.state.update(dt_ms);
    }

    /// Route a single input event into the bar and (if open) panels.
    /// Returns an optional message for the host launcher (e.g. DismissPanels).
    pub fn handle_event(&mut self, ev: &Event) -> Option<ActionBarMsg> {
        self.state.handle_event(ev)
    }

    /// Render the 35dp bar itself. This should be drawn ABOVE the wallpaper,
    /// but BELOW normal windows; the WM inset keeps windows away from it.
    /// `y` is typically 0, but kept flexible for future multi-bar setups.
    pub fn render_bar<R: Renderer>(&mut self, win: &mut R, y: i32, w: u32) {
        ui::bar::render(&mut self.state, win, y, w);
    }

    /// Render overlay panels (Notifications / Control Center) ABOVE windows.
    pub fn render_panels<R: Renderer>(&mut self, win: &mut R, screen_w: u32, screen_h: u32) {
        panels::render(&mut self.state, win, screen_w, screen_h);
    }

    /// True if any panel is open.
    pub fn any_panel_open(&self) -> bool {
        self.state.any_panel_open()
    }

    /// Close all panels (e.g. when ESC is pressed by the launcher).
    pub fn dismiss_panels(&mut self) {
        self.state.dismiss_panels();
    }
}

// Re-exports for convenience
pub use config::Config;
pub use ui::state::ActionBarMsg;
