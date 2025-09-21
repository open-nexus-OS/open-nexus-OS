// src/ui/actionbar_handler.rs
// ActionBar event handling and management - extracted from bar_handler.rs

use std::time::{Duration, Instant};
use std::os::unix::io::AsRawFd;

use orbclient::{Window, WindowFlag, Renderer};
use nexus_actionbar::{ActionBar, ActionBarMsg, Config as ActionBarConfig};
use libnexus::RedoxAnimationTimer;

use crate::dpi_scale;
use crate::config::settings::set_top_inset;
use crate::types::state::{WindowZOrder, WindowState};
use crate::services::process_manager::wait;

use log::{debug, error};

/// ActionBar handler for managing top bar events and panels
pub struct ActionBarHandler {
    actionbar: ActionBar,
    animation_timer: RedoxAnimationTimer,
    window_state: WindowState,
    actionbar_id: usize,
    panels_id: usize,

    // Panel visibility state with fade-out grace
    panels_visible: bool,
    panels_fadeout_deadline: Option<Instant>,
}

impl ActionBarHandler {
    pub fn new(width: u32, height: u32) -> Self {
        let dpi = dpi_scale();
        let mut actionbar = ActionBar::new(ActionBarConfig::default());
        let insets = actionbar.required_insets(width, height, dpi);

        // Initialize animation timer
        let mut animation_timer = RedoxAnimationTimer::new();
        debug!("Starting RedoxAnimationTimer for ActionBar...");

        animation_timer.set_callback(|| {
            debug!("RedoxAnimationTimer callback - Animation frame update");
        });

        animation_timer.start();
        debug!("RedoxAnimationTimer started successfully");

        // Set global top inset
        set_top_inset(insets.top);

        // Initialize window state
        let mut window_state = WindowState::new();

        // Create ActionBar window (always at top)
        let actionbar_id = window_state.get_next_window_id();
        let actionbar_win = Window::new_flags(
            0, 0, width, insets.top,
            "NexusActionBar",
            &[WindowFlag::Async, WindowFlag::Borderless],
        ).expect("actionbar: failed to open window");

        window_state.add_window(actionbar_id, actionbar_win, WindowZOrder::AlwaysOnTop, 0);

        // Create panels overlay window (drawn above normal windows)
        let panels_id = window_state.get_next_window_id();
        let mut panels_win = Window::new_flags(
            0, 0, width, height,
            "NexusPanels",
            &[WindowFlag::Async, WindowFlag::Borderless, WindowFlag::Transparent],
        ).expect("actionbar panels: failed to open window");

        // Keep panels window off-screen until needed
        panels_win.set_pos(-10_000, -10_000);
        panels_win.set_size(1, 1);

        window_state.add_window(panels_id, panels_win, WindowZOrder::AlwaysOnTop, 1);

        ActionBarHandler {
            actionbar,
            animation_timer,
            window_state,
            actionbar_id,
            panels_id,
            panels_visible: false,
            panels_fadeout_deadline: None,
        }
    }

    /// Initialize ActionBar rendering
    pub fn initialize(&mut self, width: u32) {
        if let Some(actionbar_window) = self.window_state.get_window_mut(self.actionbar_id) {
            actionbar_window.set(orbclient::Color::rgba(0, 0, 0, 0));
            self.actionbar.render_bar(actionbar_window, 0, width);
            actionbar_window.sync();
        }
    }

    /// Handle timer events (called from main event loop)
    pub fn handle_timer_event(&mut self, width: u32, height: u32) {
        // Update ActionBar animations
        self.actionbar.update(0);

        // Render ActionBar
        if let Some(actionbar_window) = self.window_state.get_window_mut(self.actionbar_id) {
            actionbar_window.set(orbclient::Color::rgba(0, 0, 0, 0));
            self.actionbar.render_bar(actionbar_window, 0, width);
            actionbar_window.sync();
        }

        // Handle panel overlay visibility with fade-out grace
        self.update_panel_visibility(width, height);
    }

    /// Handle ActionBar-specific events
    pub fn handle_actionbar_event(&mut self, width: u32, height: u32) -> Option<ActionBarMsg> {
        debug!("Event::ActBar - Processing ActionBar events");

        if let Some(actionbar_window) = self.window_state.get_window_mut(self.actionbar_id) {
            let mut event_count = 0;
            let mut result_msg = None;

            for ev_win in actionbar_window.events() {
                event_count += 1;
                debug!("ActionBar window event #{}: {:?}", event_count, ev_win);

                // Handle events and collect messages
                if let Some(msg) = self.actionbar.handle_event(&ev_win) {
                    debug!("ActionBar message: {:?}", msg);
                    match msg {
                        ActionBarMsg::DismissPanels => {
                            // Handled by visibility policy
                        }
                        ActionBarMsg::RequestInsetUpdate(new_insets) => {
                            actionbar_window.set_size(width, new_insets.top);
                            set_top_inset(new_insets.top);
                            result_msg = Some(msg);
                        }
                    }
                }
            }

            if event_count == 0 {
                debug!("ActionBar window: NO EVENTS received");
            }

            result_msg
        } else {
            None
        }
    }

    /// Handle screen resize events
    pub fn handle_screen_resize(&mut self, width: u32, height: u32) {
        // Update ActionBar insets and window size
        let dpi = dpi_scale();
        let insets = self.actionbar.required_insets(width, height, dpi);
        set_top_inset(insets.top);

        if let Some(w) = self.window_state.get_window_mut(self.actionbar_id) {
            w.set_pos(0, 0);
            w.set_size(width, insets.top);
        }

        // Apply current overlay visibility policy on resize
        self.update_panel_visibility(width, height);
    }

    /// Update panel overlay visibility with fade-out logic
    fn update_panel_visibility(&mut self, width: u32, height: u32) {
        const PANEL_FADEOUT_HOLD_MS: u64 = 240;
        let now = Instant::now();

        let any_animation_running = self.actionbar.is_animating();
        let want_visible = if self.actionbar.any_panel_open() || any_animation_running {
            self.panels_fadeout_deadline = None;
            true
        } else {
            if self.panels_fadeout_deadline.is_none() {
                self.panels_fadeout_deadline = Some(now + Duration::from_millis(PANEL_FADEOUT_HOLD_MS));
            }
            self.panels_fadeout_deadline.unwrap() > now
        };

        if want_visible && !self.panels_visible {
            if let Some(panels_window) = self.window_state.get_window_mut(self.panels_id) {
                panels_window.set_pos(0, 0);
                panels_window.set_size(width, height);
            }
            self.panels_visible = true;
        } else if !want_visible && self.panels_visible {
            if let Some(panels_window) = self.window_state.get_window_mut(self.panels_id) {
                panels_window.set_pos(-10_000, -10_000);
                panels_window.set_size(1, 1);
            }
            self.panels_visible = false;
        }

        // Render overlay panels (if visible)
        if self.panels_visible {
            if let Some(panels_window) = self.window_state.get_window_mut(self.panels_id) {
                panels_window.set(orbclient::Color::rgba(0, 0, 0, 0));
                self.actionbar.render_panels(panels_window, width, height);
                panels_window.sync();
            }
        }
    }

    /// Force dismiss all panels (e.g., when Start menu opens)
    pub fn dismiss_panels(&mut self) {
        self.actionbar.dismiss_panels();
        self.panels_fadeout_deadline = None;
        if self.panels_visible {
            if let Some(panels_window) = self.window_state.get_window_mut(self.panels_id) {
                panels_window.set_pos(-10_000, -10_000);
                panels_window.set_size(1, 1);
            }
            self.panels_visible = false;
        }
    }

    /// Get ActionBar window file descriptor for event subscription
    pub fn get_actionbar_fd(&self) -> i32 {
        self.window_state.get_window(self.actionbar_id)
            .map(|w| w.as_raw_fd())
            .unwrap_or(-1)
    }

    /// Get panels window file descriptor for event subscription
    pub fn get_panels_fd(&self) -> i32 {
        self.window_state.get_window(self.panels_id)
            .map(|w| w.as_raw_fd())
            .unwrap_or(-1)
    }

    /// Check if any panel is currently open
    pub fn any_panel_open(&self) -> bool {
        self.actionbar.any_panel_open()
    }

    /// Check if any animation is running
    pub fn is_animating(&self) -> bool {
        self.actionbar.is_animating()
    }

    /// Cleanup resources
    pub fn cleanup(&mut self) {
        debug!("Stopping RedoxAnimationTimer...");
        self.animation_timer.stop();
        debug!("ActionBar handler cleanup completed");
    }
}
