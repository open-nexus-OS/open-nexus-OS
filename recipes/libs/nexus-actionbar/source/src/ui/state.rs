//! Action Bar state, event handling and cross-module messages.

use orbclient::{Event, EventOption, Color};

/// Re-export Event for external use
pub use orbclient::Event as ActionBarEvent;
use crate::config::Config;
use libnexus::ui::layout::conversion::dp_to_px;
use libnexus::{Timeline, Direction, AnimationManager};
use libnexus::ui::AnimationId;

/// Messages the bar can send to the launcher.
#[derive(Clone, Debug)]
pub enum ActionBarMsg {
    /// Host should consider closing panels (e.g. click outside).
    DismissPanels,
    /// Insets changed (future-proof; useful if bar height gets dynamic).
    RequestInsetUpdate(libnexus::ui::Insets),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelOpen {
    None,
    Notifications,
    ControlCenter,
}

/// State machine for toggle button operations to prevent race conditions
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleState {
    Idle,                    // No operation in progress
    OpeningNotifications,    // Notifications panel opening
    OpeningControlCenter,    // Control center panel opening
    ClosingNotifications,    // Notifications panel closing
    ClosingControlCenter,    // Control center panel closing
}

pub struct ActionBarState {
    pub cfg: Config,
    pub dpi: f32,

    // Derived pixel sizes (updated in render/layout calls)
    pub bar_h_px: u32,

    // Buttons
    pub btn_notifications: super::buttons::Button,
    pub btn_control: super::buttons::Button,

    // Panels
    pub open: PanelOpen,
    pub tl_notifications: Timeline,
    pub tl_control: Timeline,

    // Cached paints (simple Colors for now; you can switch to THEME paints)
    pub bar_bg: Color,
    pub hover_veil: Color,
    pub panel_bg: Color,

    // Last mouse state for click detection
    last_mouse_down: bool,

    // AnimationManager integration
    notifications_animation_id: Option<AnimationId>,
    control_animation_id: Option<AnimationId>,

    // State machine for toggle operations
    toggle_state: ToggleState,
}

impl ActionBarState {
    pub fn new(cfg: Config) -> Self {
        let bar_bg = cfg.bar_bg.unwrap_or(Color::rgba(255,255,255,191));
        let hover_veil = cfg.button_hover_veil.unwrap_or(Color::rgba(0,0,0,26));
        let panel_bg = cfg.panel_bg.unwrap_or(Color::rgba(0,0,0,64));

        Self {
            dpi: 1.0,
            bar_h_px: 0,

            btn_notifications: super::buttons::Button::new(cfg.icon_notifications.clone()),
            btn_control: super::buttons::Button::new(cfg.icon_control_center.clone()),

            open: PanelOpen::None,
                   tl_notifications: Timeline::with_config(cfg.anim_duration_ms, cfg.easing),
                   tl_control: Timeline::with_config(cfg.anim_duration_ms, cfg.easing),

            bar_bg,
            hover_veil,
            panel_bg,

            cfg,
            last_mouse_down: false,

            // AnimationManager integration - will be set later
            notifications_animation_id: None,
            control_animation_id: None,

            // State machine starts in idle
            toggle_state: ToggleState::Idle,
        }
    }

    pub fn any_panel_open(&self) -> bool {
        !matches!(self.open, PanelOpen::None)
    }

    /// Set the AnimationManager for 60fps animations.
    /// This registers the timelines with the manager and stores their IDs.
    pub fn set_animation_manager(&mut self, manager: &mut AnimationManager) {
        // Register timelines with AnimationManager
        self.notifications_animation_id = Some(manager.add_timeline(self.tl_notifications.clone()));
        self.control_animation_id = Some(manager.add_timeline(self.tl_control.clone()));
    }

    pub fn dismiss_panels(&mut self) {
        // Only dismiss if not currently in a toggle operation
        if self.toggle_state != ToggleState::Idle {
            return;
        }

        self.open = PanelOpen::None;
        self.btn_notifications.pressed = false;
        self.btn_control.pressed = false;
        self.tl_notifications.start(Direction::Out);
        self.tl_control.start(Direction::Out);
        if self.cfg.reduced_motion {
            self.tl_notifications.set_immediate(false);
            self.tl_control.set_immediate(false);
        }
    }

    pub fn update(&mut self, _dt_ms: u32) {
        // AnimationManager now handles timeline updates at 60fps
        // Only do state validation here
        self.check_animation_stuck_states();
        self.update_toggle_state();
    }

    /// Update toggle state machine based on animation progress
    fn update_toggle_state(&mut self) {
        let old_state = self.toggle_state;
        match self.toggle_state {
            ToggleState::OpeningNotifications => {
                if self.tl_notifications.value() >= 0.99 {
                    self.toggle_state = ToggleState::Idle;
                    log::debug!("State-Machine: OpeningNotifications -> Idle (value: {:.3})", self.tl_notifications.value());
                }
            }
            ToggleState::OpeningControlCenter => {
                if self.tl_control.value() >= 0.99 {
                    self.toggle_state = ToggleState::Idle;
                    log::debug!("State-Machine: OpeningControlCenter -> Idle (value: {:.3})", self.tl_control.value());
                }
            }
            ToggleState::ClosingNotifications => {
                if self.tl_notifications.value() <= 0.01 {
                    self.toggle_state = ToggleState::Idle;
                    log::debug!("State-Machine: ClosingNotifications -> Idle (value: {:.3})", self.tl_notifications.value());
                }
            }
            ToggleState::ClosingControlCenter => {
                if self.tl_control.value() <= 0.01 {
                    self.toggle_state = ToggleState::Idle;
                    log::debug!("State-Machine: ClosingControlCenter -> Idle (value: {:.3})", self.tl_control.value());
                }
            }
            ToggleState::Idle => {
                // Already idle, nothing to do
            }
        }
        if old_state != self.toggle_state {
            log::debug!("State-Machine: {:?} -> {:?}", old_state, self.toggle_state);
        }
    }

    /// Check if any animation is currently running.
    pub fn is_animating(&self) -> bool {
        self.tl_notifications.is_running() || self.tl_control.is_running()
    }

    /// Handle input for the bar + panels, return optional message to host.
    pub fn handle_event(&mut self, ev: &Event) -> Option<ActionBarMsg> {
        match ev.to_option() {
            EventOption::Mouse(m) => {
                // Hover states
                self.btn_notifications.hover = self.btn_notifications.hit(m.x, m.y);
                self.btn_control.hover      = self.btn_control.hit(m.x, m.y);
                None
            }
            EventOption::Button(b) => {
                let down = b.left;

                // Edge: on release, commit click if cursor is still inside a button.
                if !down && self.last_mouse_down {
                    if self.btn_notifications.hover {
                        self.toggle_notifications();
                        // Validate state after toggle
                        self.validate_state();
                        return None;
                    } else if self.btn_control.hover {
                        self.toggle_control_center();
                        // Validate state after toggle
                        self.validate_state();
                        return None;
                    } else if self.any_panel_open() && self.toggle_state == ToggleState::Idle {
                        // Click outside any control while a panel is open -> request dismiss.
                        // Only if not currently in a toggle operation
                        self.dismiss_panels();
                        return Some(ActionBarMsg::DismissPanels);
                    }
                }
                self.last_mouse_down = down;
                None
            }
            _ => None,
        }
    }

    pub fn toggle_notifications(&mut self) {
        // Only allow toggle if not currently in a toggle operation
        if self.toggle_state != ToggleState::Idle {
            log::debug!("Toggle-Notifications: BLOCKED! State is {:?}", self.toggle_state);
            return;
        }

        let new_on = !self.btn_notifications.pressed;
        log::debug!("Toggle-Notifications: {} -> {} (state: {:?})",
                   self.btn_notifications.pressed, new_on, self.toggle_state);

        self.btn_notifications.pressed = new_on;
        self.btn_control.pressed = false;
        self.open = if new_on { PanelOpen::Notifications } else { PanelOpen::None };

        // Set state machine
        self.toggle_state = if new_on {
            ToggleState::OpeningNotifications
        } else {
            ToggleState::ClosingNotifications
        };

        self.tl_notifications.start(if new_on { Direction::In } else { Direction::Out });
        self.tl_control.start(Direction::Out);

        if self.cfg.reduced_motion {
            self.tl_notifications.set_immediate(new_on);
            self.tl_control.set_immediate(false);
            // If reduced motion, immediately go back to idle
            self.toggle_state = ToggleState::Idle;
        }

        log::debug!("Toggle-Notifications: New state: {:?}, timeline running: {}",
                   self.toggle_state, self.tl_notifications.is_running());
    }

    pub fn toggle_control_center(&mut self) {
        // Only allow toggle if not currently in a toggle operation
        if self.toggle_state != ToggleState::Idle {
            log::debug!("Toggle-ControlCenter: BLOCKED! State is {:?}", self.toggle_state);
            return;
        }

        let new_on = !self.btn_control.pressed;
        log::debug!("Toggle-ControlCenter: {} -> {} (state: {:?})",
                   self.btn_control.pressed, new_on, self.toggle_state);

        self.btn_control.pressed = new_on;
        self.btn_notifications.pressed = false;
        self.open = if new_on { PanelOpen::ControlCenter } else { PanelOpen::None };

        // Set state machine
        self.toggle_state = if new_on {
            ToggleState::OpeningControlCenter
        } else {
            ToggleState::ClosingControlCenter
        };

        self.tl_control.start(if new_on { Direction::In } else { Direction::Out });
        self.tl_notifications.start(Direction::Out);

        if self.cfg.reduced_motion {
            self.tl_control.set_immediate(new_on);
            self.tl_notifications.set_immediate(false);
            // If reduced motion, immediately go back to idle
            self.toggle_state = ToggleState::Idle;
        }

        log::debug!("Toggle-ControlCenter: New state: {:?}, timeline running: {}",
                   self.toggle_state, self.tl_control.is_running());
    }

    /// Validate and fix state inconsistencies
    pub fn validate_state(&mut self) {
        // Ensure button states match panel state
        match self.open {
            PanelOpen::Notifications => {
                if !self.btn_notifications.pressed {
                    self.btn_notifications.pressed = true;
                }
                if self.btn_control.pressed {
                    self.btn_control.pressed = false;
                }
            }
            PanelOpen::ControlCenter => {
                if !self.btn_control.pressed {
                    self.btn_control.pressed = true;
                }
                if self.btn_notifications.pressed {
                    self.btn_notifications.pressed = false;
                }
            }
            PanelOpen::None => {
                if self.btn_notifications.pressed {
                    self.btn_notifications.pressed = false;
                }
                if self.btn_control.pressed {
                    self.btn_control.pressed = false;
                }
            }
        }

        // Ensure timeline directions match button states
        if self.btn_notifications.pressed && self.tl_notifications.direction != Direction::In {
            self.tl_notifications.start(Direction::In);
        } else if !self.btn_notifications.pressed && self.tl_notifications.direction != Direction::Out {
            self.tl_notifications.start(Direction::Out);
        }

        if self.btn_control.pressed && self.tl_control.direction != Direction::In {
            self.tl_control.start(Direction::In);
        } else if !self.btn_control.pressed && self.tl_control.direction != Direction::Out {
            self.tl_control.start(Direction::Out);
        }
    }

    /// Check for and recover from stuck animation states
    fn check_animation_stuck_states(&mut self) {
        // If animation has been running too long without reaching target, force completion
        const MAX_ANIMATION_MS: u32 = 5000; // 5 seconds max

        // Check notifications timeline
        if self.btn_notifications.pressed && self.tl_notifications.value() < 0.99 {
            // Animation should be complete but isn't - force it
            self.tl_notifications.set_immediate(true);
        } else if !self.btn_notifications.pressed && self.tl_notifications.value() > 0.01 {
            // Animation should be closed but isn't - force it
            self.tl_notifications.set_immediate(false);
        }

        // Check control center timeline
        if self.btn_control.pressed && self.tl_control.value() < 0.99 {
            // Animation should be complete but isn't - force it
            self.tl_control.set_immediate(true);
        } else if !self.btn_control.pressed && self.tl_control.value() > 0.01 {
            // Animation should be closed but isn't - force it
            self.tl_control.set_immediate(false);
        }
    }

    /// Update derived pixel sizes and button rectangles.
    pub fn layout_bar(&mut self, dpi: f32, bar_y: i32, bar_w: u32) {
        self.dpi = dpi;
        self.bar_h_px = dp_to_px(self.cfg.height_dp, dpi);

        let slot = libnexus::ui::layout::button_slot_px(self.bar_h_px);
        let icon_px = (self.bar_h_px as f32 * 0.66).round() as u32;

        // Left button (notifications)
        let left_rect = (8, bar_y + ((self.bar_h_px as i32 - slot as i32) / 2), slot as i32, slot as i32);
        // Right button (control center)
        let right_x = (bar_w as i32 - slot as i32 - 8).max(0);
        let right_rect = (right_x, bar_y + ((self.bar_h_px as i32 - slot as i32) / 2), slot as i32, slot as i32);

        self.btn_notifications.set_rect(left_rect);
        self.btn_control.set_rect(right_rect);

        // Preload icon at target px (optional)
        let _ = icon_px;
    }
}
