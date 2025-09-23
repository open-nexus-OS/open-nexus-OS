// src/ui/actionbar_handler.rs
// ActionBar event handling and management — immediate toggle (no animations).
// Provides: initialize(), process_events(), render_now(), handle_screen_resize(), dismiss_panels().

use std::time::{Duration, Instant};
use std::os::unix::io::AsRawFd;

use orbclient::{Window, WindowFlag, Renderer};
use nexus_actionbar::{ActionBar, ActionBarMsg, Config as ActionBarConfig};
use libnexus::RedoxAnimationTimer;

use crate::dpi_scale;
use crate::config::settings::set_top_inset;
use crate::types::state::{WindowZOrder, WindowState};
// remove: use crate::services::process_manager::wait;

use log::{debug, error};

/// ActionBar handler for managing top bar events and panels
pub struct ActionBarHandler {
    actionbar: ActionBar,
    // animation_timer: RedoxAnimationTimer, // not needed for "no animation" mode
    window_state: WindowState,
    actionbar_id: usize,
    panels_id: usize,

    // Panel visibility state
    panels_visible: bool,

    // Track latest screen size and a coarse "last update" moment
    screen_w: u32,
    screen_h: u32,
    last_update: Instant,
}

impl ActionBarHandler {
    pub fn new(width: u32, height: u32) -> Self {
        let dpi = dpi_scale();
        let mut actionbar = ActionBar::new(ActionBarConfig::default());
        let insets = actionbar.required_insets(width, height, dpi);

        // We intentionally do not run a high-frequency animation timer.
        // Instead, we will "fast-forward" the state machine in handle_timer_event().
        set_top_inset(insets.top);

        let mut window_state = WindowState::new();

        // Top bar window
        let actionbar_id = window_state.get_next_window_id();
        let actionbar_win = Window::new_flags(
            0, 0, width, insets.top,
            "NexusActionBar",
            &[WindowFlag::Async, WindowFlag::Borderless],
        ).expect("actionbar: failed to open window");
        window_state.add_window(actionbar_id, actionbar_win, WindowZOrder::AlwaysOnTop, 0);

        // Panels overlay window (kept off-screen until visible)
        let panels_id = window_state.get_next_window_id();
        let mut panels_win = Window::new_flags(
            0, 0, width, height,
            "NexusPanels",
            &[WindowFlag::Async, WindowFlag::Borderless, WindowFlag::Transparent],
        ).expect("actionbar panels: failed to open window");
        panels_win.set_pos(-10_000, -10_000);
        panels_win.set_size(1, 1);
        window_state.add_window(panels_id, panels_win, WindowZOrder::AlwaysOnTop, 1);

        Self {
            actionbar,
            window_state,
            actionbar_id,
            panels_id,
            panels_visible: false,
            screen_w: width,
            screen_h: height,
            last_update: Instant::now(),
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

    /// Drive the actionbar state-machine forward with a large dt to "skip" animations.
    fn fast_forward(&mut self) {
        // We don't rely on elapsed time; we always jump enough to complete any opening/closing.
        // If your timelines are < 1000ms this will finish in one call.
        self.actionbar.update(1000);
        self.last_update = Instant::now();
    }

    /// Handle timer events (called from main event loop every ~1s)
    pub fn handle_timer_event(&mut self, width: u32, height: u32) {
        // Track current screen size
        self.screen_w = width;
        self.screen_h = height;

        // Fast-forward the state machine to complete any in-progress toggle.
        self.fast_forward();

        // Render bar
        if let Some(actionbar_window) = self.window_state.get_window_mut(self.actionbar_id) {
            actionbar_window.set(orbclient::Color::rgba(0, 0, 0, 0));
            self.actionbar.render_bar(actionbar_window, 0, width);
            actionbar_window.sync();
        }

        // Apply/show panels overlay and render it (if needed)
        self.update_panel_visibility(width, height);
    }

    /// Handle ActionBar-specific window events (mouse/keys routed to the actionbar widget)
    pub fn handle_actionbar_event(&mut self, width: u32, height: u32) -> Option<ActionBarMsg> {
        if let Some(actionbar_window) = self.window_state.get_window_mut(self.actionbar_id) {
            let mut result_msg = None;

            for ev_win in actionbar_window.events() {
                if let Some(msg) = self.actionbar.handle_event(&ev_win) {
                    result_msg = Some(msg);
                }
            }

            // After processing events, force-complete any transitions
            self.fast_forward();
            // And re-render
            if let Some(w) = self.window_state.get_window_mut(self.actionbar_id) {
                w.set(orbclient::Color::rgba(0,0,0,0));
                self.actionbar.render_bar(w, 0, width);
                w.sync();
            }
            self.update_panel_visibility(width, height);

            result_msg
        } else {
            None
        }
    }

    /// Handle screen resize events
    pub fn handle_screen_resize(&mut self, width: u32, height: u32) {
        self.screen_w = width;
        self.screen_h = height;

        let dpi = dpi_scale();
        let insets = self.actionbar.required_insets(width, height, dpi);
        set_top_inset(insets.top);

        if let Some(w) = self.window_state.get_window_mut(self.actionbar_id) {
            w.set_pos(0, 0);
            w.set_size(width, insets.top);
        }

        self.update_panel_visibility(width, height);
    }

    /// Update panel overlay visibility – immediate show/hide, no fades
    fn update_panel_visibility(&mut self, width: u32, height: u32) {
        let want_visible = self.actionbar.any_panel_open();

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

        // If visible, render the panels immediately
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
        if self.panels_visible {
            if let Some(panels_window) = self.window_state.get_window_mut(self.panels_id) {
                panels_window.set_pos(-10_000, -10_000);
                panels_window.set_size(1, 1);
            }
            self.panels_visible = false;
        }
        // Make sure state-machine lands in a stable "closed" state
        self.fast_forward();
    }

    /// Get ActionBar / Panels windows (unchanged)
    pub fn get_actionbar_fd(&self) -> i32 {
        self.window_state.get_window(self.actionbar_id)
            .map(|w| w.as_raw_fd())
            .unwrap_or(-1)
    }
    pub fn get_panels_fd(&self) -> i32 {
        self.window_state.get_window(self.panels_id)
            .map(|w| w.as_raw_fd())
            .unwrap_or(-1)
    }
    pub fn any_panel_open(&self) -> bool { self.actionbar.any_panel_open() }
    pub fn is_animating(&self) -> bool { self.actionbar.is_animating() }

    pub fn get_actionbar_window(&mut self) -> Option<&mut Window> {
        self.window_state.get_window_mut(self.actionbar_id)
    }
    pub fn get_panels_window(&mut self) -> Option<&mut Window> {
        self.window_state.get_window_mut(self.panels_id)
    }

    /// Direct mouse event path (when you route events yourself)
    pub fn handle_mouse_event(&mut self, x: i32, y: i32) {
        let mouse_event = orbclient::Event { code: orbclient::EVENT_MOUSE, a: x as i64, b: y as i64 };
        let _ = self.actionbar.handle_event(&mouse_event);
        self.fast_forward();
        self.update_panel_visibility(self.screen_w, self.screen_h);
    }

    /// Direct button click path (when you route events yourself)
    pub fn handle_button_click(&mut self, button: u8, x: i32, y: i32) {
        if button == 1 {
            let button_event = orbclient::Event { code: orbclient::EVENT_BUTTON, a: x as i64, b: y as i64 };
            let _ = self.actionbar.handle_event(&button_event);
            self.fast_forward();
            self.update_panel_visibility(self.screen_w, self.screen_h);
        }
    }

    /// Compatibility shim used by bar_handler: re-render immediately.
    /// Fast-forwards the internal state-machine so any open/close completes
    /// without visible animation, then paints bar + panels once.
    pub fn render_now(&mut self, width: u32, height: u32) {
        // Remember current screen size for subsequent calls
        self.screen_w = width;
        self.screen_h = height;

        // Complete any in-flight transitions (no animations)
        self.fast_forward();

        // Render the top bar
        if let Some(actionbar_window) = self.window_state.get_window_mut(self.actionbar_id) {
            actionbar_window.set(orbclient::Color::rgba(0, 0, 0, 0));
            self.actionbar.render_bar(actionbar_window, 0, width);
            actionbar_window.sync();
        }

        // Show/hide + render overlay panels as needed
        self.update_panel_visibility(width, height);
    }

    /// Compatibility shim used by bar_handler: poll both actionbar and panels windows,
    /// feed events into the actionbar widget, fast-forward, then paint everything.
    pub fn process_events(&mut self, width: u32, height: u32) -> Option<ActionBarMsg> {
        let mut last_msg: Option<ActionBarMsg> = None;
        self.screen_w = width;
        self.screen_h = height;

        // Drain events from the actionbar window
        if let Some(actionbar_window) = self.window_state.get_window_mut(self.actionbar_id) {
            for ev in actionbar_window.events() {
                if let Some(msg) = self.actionbar.handle_event(&ev) {
                    last_msg = Some(msg);
                }
            }
        }

        // Drain events from the panels overlay window (click outside to dismiss, etc.)
        if let Some(panels_window) = self.window_state.get_window_mut(self.panels_id) {
            for ev in panels_window.events() {
                if let Some(msg) = self.actionbar.handle_event(&ev) {
                    last_msg = Some(msg);
                }
            }
        }

        // Complete state transitions instantly and render
        self.fast_forward();

        if let Some(actionbar_window) = self.window_state.get_window_mut(self.actionbar_id) {
            actionbar_window.set(orbclient::Color::rgba(0, 0, 0, 0));
            self.actionbar.render_bar(actionbar_window, 0, width);
            actionbar_window.sync();
        }

        self.update_panel_visibility(width, height);

        last_msg
    }

    pub fn cleanup(&mut self) {}
}
