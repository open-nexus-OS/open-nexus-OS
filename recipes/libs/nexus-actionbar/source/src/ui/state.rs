//! Action Bar state, event handling and cross-module messages.

use orbclient::{Event, EventOption, Color};
use crate::config::Config;
use super::animation::{Timeline, Direction};
use super::layout::{dp_to_px, button_slot_px};

/// Messages the bar can send to the launcher.
#[derive(Clone, Debug)]
pub enum ActionBarMsg {
    /// Host should consider closing panels (e.g. click outside).
    DismissPanels,
    /// Insets changed (future-proof; useful if bar height gets dynamic).
    RequestInsetUpdate(super::layout::Insets),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelOpen {
    None,
    Notifications,
    ControlCenter,
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
            tl_notifications: Timeline::new(cfg.anim_duration_ms, cfg.easing),
            tl_control: Timeline::new(cfg.anim_duration_ms, cfg.easing),

            bar_bg,
            hover_veil,
            panel_bg,

            cfg,
            last_mouse_down: false,
        }
    }

    pub fn any_panel_open(&self) -> bool {
        !matches!(self.open, PanelOpen::None)
    }

    pub fn dismiss_panels(&mut self) {
        self.open = PanelOpen::None;
        self.btn_notifications.pressed = false;
        self.btn_control.pressed = false;
        self.tl_notifications.set_dir(Direction::Out);
        self.tl_control.set_dir(Direction::Out);
        if self.cfg.reduced_motion {
            self.tl_notifications.set_immediate(false);
            self.tl_control.set_immediate(false);
        }
    }

    pub fn update(&mut self, dt_ms: u32) {
        self.tl_notifications.tick(dt_ms);
        self.tl_control.tick(dt_ms);
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
                        return None;
                    } else if self.btn_control.hover {
                        self.toggle_control_center();
                        return None;
                    } else if self.any_panel_open() {
                        // Click outside any control while a panel is open -> request dismiss.
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
        let new_on = !self.btn_notifications.pressed;
        self.btn_notifications.pressed = new_on;
        self.btn_control.pressed = false;
        self.open = if new_on { PanelOpen::Notifications } else { PanelOpen::None };

        self.tl_notifications.set_dir(if new_on { Direction::In } else { Direction::Out });
        self.tl_control.set_dir(Direction::Out);

        if self.cfg.reduced_motion {
            self.tl_notifications.set_immediate(new_on);
            self.tl_control.set_immediate(false);
        }
    }

    pub fn toggle_control_center(&mut self) {
        let new_on = !self.btn_control.pressed;
        self.btn_control.pressed = new_on;
        self.btn_notifications.pressed = false;
        self.open = if new_on { PanelOpen::ControlCenter } else { PanelOpen::None };

        self.tl_control.set_dir(if new_on { Direction::In } else { Direction::Out });
        self.tl_notifications.set_dir(Direction::Out);

        if self.cfg.reduced_motion {
            self.tl_control.set_immediate(new_on);
            self.tl_notifications.set_immediate(false);
        }
    }

    /// Update derived pixel sizes and button rectangles.
    pub fn layout_bar(&mut self, dpi: f32, bar_y: i32, bar_w: u32) {
        self.dpi = dpi;
        self.bar_h_px = dp_to_px(self.cfg.height_dp, dpi);

        let slot = super::layout::button_slot_px(self.bar_h_px);
        let icon_px = (self.bar_h_px as f32 * 0.66).round() as u32;

        // Left button (notifications)
        let left_rect = (8, bar_y + ((self.bar_h_px as i32 - slot) / 2), slot, slot);
        // Right button (control center)
        let right_x = (bar_w as i32 - slot - 8).max(0);
        let right_rect = (right_x, bar_y + ((self.bar_h_px as i32 - slot) / 2), slot, slot);

        self.btn_notifications.set_rect(left_rect);
        self.btn_control.set_rect(right_rect);

        // Preload icon at target px (optional)
        let _ = icon_px;
    }
}
