//! Bar / panel defaults and external configuration surface.
//! Colors and icon names are injected; paints are resolved via libnexus THEME.

use orbclient::Color;
use libnexus::themes::{THEME, Paint};
use libnexus::Easing;

/// UI layout mode (mirrors the launcher’s notion, kept local for decoupling).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UIMode { Desktop, Mobile }

/// Theme mode (mirrors THEME’s Light/Dark, kept local for decoupling).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ThemeMode { Light, Dark }

/// Public configuration the launcher passes in.
/// All sizes are logical **dp**; conversion to px happens in layout/panels.
#[derive(Clone, Debug)]
pub struct Config {
    pub height_dp: u32,            // default 35
    pub reduced_motion: bool,
    pub anim_duration_ms: u32,
    pub easing: Easing,

    /// Icon IDs (resolved by libnexus::THEME)
    pub icon_notifications: String,
    pub icon_control_center: String,

    /// Optional color overrides; None → resolved via THEME keys at render time.
    pub bar_bg: Option<Color>,
    pub button_hover_veil: Option<Color>,
    pub panel_bg: Option<Color>,

    /// Panel widths in dp (clamped at runtime to screen)
    pub notifications_width_dp: u32,
    pub control_center_width_dp: u32,

    /// Bottom gap so side panels do not overlap a bottom launcher/taskbar.
    /// Desktop usually has a bottom bar; Mobile usually not.
    pub bottom_gap_desktop_dp: u32,
    pub bottom_gap_mobile_dp: u32,

    /// Initial modes (ActionBar keeps its own copy to render toggles).
    pub initial_ui_mode: UIMode,
    pub initial_theme_mode: ThemeMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            height_dp: 35,
            reduced_motion: false,
            anim_duration_ms: 250,
            easing: Easing::CubicOut,

            icon_notifications: "notifications.button".into(),
            icon_control_center: "controlcenter.button".into(),

            bar_bg: None,
            button_hover_veil: None,
            panel_bg: None,

            notifications_width_dp: 350,
            control_center_width_dp: 350,

            // Default: Desktop keeps space for a ~54dp bottom bar; Mobile is fullscreen.
            bottom_gap_desktop_dp: 54,
            bottom_gap_mobile_dp: 0,

            initial_ui_mode: UIMode::Desktop,
            initial_theme_mode: ThemeMode::Light,
        }
    }
}

// -------- THEME convenience (paints may include acrylic) --------

pub fn bar_bg_paint() -> Paint {
    THEME.paint("actionbar_bg", Paint {
        color: Color::rgba(255, 255, 255, 89),
        acrylic: None
    })
}

pub fn panel_bg_paint() -> Paint {
    // Generic panel fallback; panels use their own keys too.
    THEME.paint("panel_bg", Paint {
        color: Color::rgba(0, 0, 0, 89),
        acrylic: None
    })
}

pub fn button_hover_veil_paint() -> Paint {
    THEME.paint("button_hover_veil", Paint {
        color: Color::rgba(0, 0, 0, 26),
        acrylic: None
    })
}

pub fn control_center_group_bg_paint() -> Paint {
    THEME.paint("control_center_group_bg", Paint {
        color: Color::rgba(255, 255, 255, 13),
        acrylic: None
    })
}

pub fn control_center_item_bg_active_paint() -> Paint {
    THEME.paint("control_center_item_bg_active", Paint {
        color: Color::rgba(255, 255, 255, 191),
        acrylic: None
    })
}

pub fn control_center_item_bg_muted_paint() -> Paint {
    THEME.paint("control_center_item_bg_muted", Paint {
        color: Color::rgba(0, 0, 0, 51),
        acrylic: None
    })
}

pub fn notification_pill_bg_paint() -> Paint {
    THEME.paint("notification_pill_bg", Paint {
        color: Color::rgba(0, 0, 0, 51),
        acrylic: None
    })
}
