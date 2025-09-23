//! Action Bar state, event handling and cross-module messages.

use orbclient::{Event, EventOption, Color};

pub use orbclient::Event as ActionBarEvent;
use crate::config::{Config, UIMode, ThemeMode};
use libnexus::ui::layout::conversion::dp_to_px;
use libnexus::{Timeline, Direction, AnimationManager};
use libnexus::ui::AnimationId;

/// Messages the bar can send to the host (launcher).
#[derive(Clone, Debug)]
pub enum ActionBarMsg {
    /// Host should consider closing panels (e.g. click outside).
    DismissPanels,
    /// Insets changed (future-proof; useful if bar height gets dynamic).
    RequestInsetUpdate(libnexus::ui::Insets),
    /// User requested a UI mode switch (desktop/mobile).
    RequestSetMode(UIMode),
    /// User requested a theme switch (light/dark).
    RequestSetTheme(ThemeMode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelOpen { None, Notifications, ControlCenter }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleState {
    Idle,
    OpeningNotifications,
    OpeningControlCenter,
    ClosingNotifications,
    ClosingControlCenter,
}

pub struct ActionBarState {
    pub cfg: Config,
    pub dpi: f32,

    // Derived pixel sizes
    pub bar_h_px: u32,

    // Buttons (on the bar)
    pub btn_notifications: super::buttons::Button,
    pub btn_control: super::buttons::Button,

    // Panels
    pub open: PanelOpen,
    pub tl_notifications: Timeline,
    pub tl_control: Timeline,

    // Cached paints (simple Colors for now)
    pub bar_bg: Color,
    pub hover_veil: Color,
    pub panel_bg: Color,

    // Current modes reflected in Control Center
    pub ui_mode: UIMode,
    pub theme_mode: ThemeMode,

    // Control Center hit areas (updated in panel render)
    pub cc_hit_mode: Option<(i32,i32,i32,i32)>,
    pub cc_hit_theme: Option<(i32,i32,i32,i32)>,

    // Last mouse state/pos for click detection
    last_mouse_down: bool,
    last_mouse_pos: (i32, i32),

    // AnimationManager integration
    notifications_animation_id: Option<AnimationId>,
    control_animation_id: Option<AnimationId>,

    // State machine
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

            ui_mode: cfg.initial_ui_mode,
            theme_mode: cfg.initial_theme_mode,

            cc_hit_mode: None,
            cc_hit_theme: None,

            cfg,
            last_mouse_down: false,
            last_mouse_pos: (0, 0),

            notifications_animation_id: None,
            control_animation_id: None,

            toggle_state: ToggleState::Idle,
        }
    }

    pub fn bottom_gap_px(&self) -> u32 {
        let dp = match self.ui_mode {
            UIMode::Desktop => self.cfg.bottom_gap_desktop_dp,
            UIMode::Mobile  => self.cfg.bottom_gap_mobile_dp,
        };
        dp_to_px(dp, self.dpi.max(1.0))
    }

    pub fn any_panel_open(&self) -> bool {
        !matches!(self.open, PanelOpen::None)
    }

    pub fn set_animation_manager(&mut self, manager: &mut AnimationManager) {
        self.notifications_animation_id = Some(manager.add_timeline(self.tl_notifications.clone()));
        self.control_animation_id = Some(manager.add_timeline(self.tl_control.clone()));
    }

    pub fn dismiss_panels(&mut self) {
        if self.toggle_state != ToggleState::Idle { return; }
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
        self.check_animation_stuck_states();
        self.update_toggle_state();
    }

    fn update_toggle_state(&mut self) {
        use ToggleState::*;
        let old = self.toggle_state;
        match self.toggle_state {
            OpeningNotifications if self.tl_notifications.value() >= 0.99 => self.toggle_state = Idle,
            OpeningControlCenter  if self.tl_control.value()       >= 0.99 => self.toggle_state = Idle,
            ClosingNotifications  if self.tl_notifications.value() <= 0.01 => self.toggle_state = Idle,
            ClosingControlCenter  if self.tl_control.value()       <= 0.01 => self.toggle_state = Idle,
            _ => {}
        }
        if old != self.toggle_state {
            log::debug!("ActionBar: {:?} -> {:?}", old, self.toggle_state);
        }
    }

    pub fn is_animating(&self) -> bool {
        self.tl_notifications.is_running() || self.tl_control.is_running()
    }

    pub fn handle_event(&mut self, ev: &Event) -> Option<ActionBarMsg> {
        match ev.to_option() {
            EventOption::Mouse(m) => {
                self.last_mouse_pos = (m.x, m.y);
                // hover states for bar buttons only
                self.btn_notifications.hover = self.btn_notifications.hit(m.x, m.y);
                self.btn_control.hover      = self.btn_control.hit(m.x, m.y);
                None
            }
            EventOption::Button(b) => {
                let down = b.left;

                // Edge: mouse release
                if !down && self.last_mouse_down {
                    let (mx, my) = self.last_mouse_pos;

                    // Bar buttons
                    if self.btn_notifications.hover {
                        self.toggle_notifications();
                        self.validate_state();
                        return None;
                    } else if self.btn_control.hover {
                        self.toggle_control_center();
                        self.validate_state();
                        return None;
                    }

                    // Panel buttons (only when Control Center is open)
                    if matches!(self.open, PanelOpen::ControlCenter) {
                        if let Some(r) = self.cc_hit_mode {
                            if hit(mx, my, r) {
                                // Toggle Desktop/Mobile
                                self.ui_mode = match self.ui_mode {
                                    UIMode::Desktop => UIMode::Mobile,
                                    UIMode::Mobile  => UIMode::Desktop,
                                };
                                return Some(ActionBarMsg::RequestSetMode(self.ui_mode));
                            }
                        }
                        if let Some(r) = self.cc_hit_theme {
                            if hit(mx, my, r) {
                                // Toggle Light/Dark
                                self.theme_mode = match self.theme_mode {
                                    ThemeMode::Light => ThemeMode::Dark,
                                    ThemeMode::Dark  => ThemeMode::Light,
                                };
                                return Some(ActionBarMsg::RequestSetTheme(self.theme_mode));
                            }
                        }
                        // Click outside known controls → dismiss
                        if self.toggle_state == ToggleState::Idle {
                            self.dismiss_panels();
                            return Some(ActionBarMsg::DismissPanels);
                        }
                    }
                }

                self.last_mouse_down = down;
                None
            }
            _ => None,
        }
    }

    pub fn toggle_notifications(&mut self) {
        if self.toggle_state != ToggleState::Idle {
            log::debug!("Toggle-Notifications: BLOCKED! {:?}", self.toggle_state);
            return;
        }
        let new_on = !self.btn_notifications.pressed;
        self.btn_notifications.pressed = new_on;
        self.btn_control.pressed = false;
        self.open = if new_on { PanelOpen::Notifications } else { PanelOpen::None };

        self.toggle_state = if new_on { ToggleState::OpeningNotifications } else { ToggleState::ClosingNotifications };
        self.tl_notifications.start(if new_on { Direction::In } else { Direction::Out });
        self.tl_control.start(Direction::Out);

        if self.cfg.reduced_motion {
            self.tl_notifications.set_immediate(new_on);
            self.tl_control.set_immediate(false);
            self.toggle_state = ToggleState::Idle;
        }
    }

    pub fn toggle_control_center(&mut self) {
        if self.toggle_state != ToggleState::Idle {
            log::debug!("Toggle-ControlCenter: BLOCKED! {:?}", self.toggle_state);
            return;
        }
        let new_on = !self.btn_control.pressed;
        self.btn_control.pressed = new_on;
        self.btn_notifications.pressed = false;
        self.open = if new_on { PanelOpen::ControlCenter } else { PanelOpen::None };

        self.toggle_state = if new_on { ToggleState::OpeningControlCenter } else { ToggleState::ClosingControlCenter };
        self.tl_control.start(if new_on { Direction::In } else { Direction::Out });
        self.tl_notifications.start(Direction::Out);

        if self.cfg.reduced_motion {
            self.tl_control.set_immediate(new_on);
            self.tl_notifications.set_immediate(false);
            self.toggle_state = ToggleState::Idle;
        }
    }

    pub fn validate_state(&mut self) {
        use Direction::*;
        match self.open {
            PanelOpen::Notifications => {
                self.btn_notifications.pressed = true;
                self.btn_control.pressed = false;
                if self.tl_notifications.direction != In { self.tl_notifications.start(In); }
                if self.tl_control.direction != Out { self.tl_control.start(Out); }
            }
            PanelOpen::ControlCenter => {
                self.btn_control.pressed = true;
                self.btn_notifications.pressed = false;
                if self.tl_control.direction != In { self.tl_control.start(In); }
                if self.tl_notifications.direction != Out { self.tl_notifications.start(Out); }
            }
            PanelOpen::None => {
                self.btn_notifications.pressed = false;
                self.btn_control.pressed = false;
                if self.tl_notifications.direction != Out { self.tl_notifications.start(Out); }
                if self.tl_control.direction != Out { self.tl_control.start(Out); }
            }
        }
    }

    fn check_animation_stuck_states(&mut self) {
        // Force-complete if reduced-motion or stale.
        if self.btn_notifications.pressed && self.tl_notifications.value() < 0.99 {
            self.tl_notifications.set_immediate(true);
        } else if !self.btn_notifications.pressed && self.tl_notifications.value() > 0.01 {
            self.tl_notifications.set_immediate(false);
        }

        if self.btn_control.pressed && self.tl_control.value() < 0.99 {
            self.tl_control.set_immediate(true);
        } else if !self.btn_control.pressed && self.tl_control.value() > 0.01 {
            self.tl_control.set_immediate(false);
        }
    }

    /// Update derived pixel sizes and button rectangles.
    pub fn layout_bar(&mut self, dpi: f32, bar_y: i32, bar_w: u32) {
        self.dpi = dpi;
        self.bar_h_px = dp_to_px(self.cfg.height_dp, dpi);

        let slot = libnexus::ui::layout::button_slot_px(self.bar_h_px);
        let _icon_px = (self.bar_h_px as f32 * 0.66).round() as u32;

        let left_rect = (8, bar_y + ((self.bar_h_px as i32 - slot as i32) / 2), slot as i32, slot as i32);
        let right_x = (bar_w as i32 - slot as i32 - 8).max(0);
        let right_rect = (right_x, bar_y + ((self.bar_h_px as i32 - slot as i32) / 2), slot as i32, slot as i32);

        self.btn_notifications.set_rect(left_rect);
        self.btn_control.set_rect(right_rect);
    }
}

#[inline]
fn hit(x: i32, y: i32, r: (i32,i32,i32,i32)) -> bool {
    let (rx, ry, rw, rh) = r;
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}
