//! Bar / panel defaults and external configuration surface.
//! Colors and icon names are injected; we resolve paints via libnexus THEME.

use orbclient::Color;
use libnexus::themes::{THEME, Paint, Acrylic};

/// Easing modes kept simple for now.
#[derive(Copy, Clone, Debug)]
pub enum Easing {
    CubicOut,
    Linear,
}

/// Public configuration the launcher passes in.
/// All sizes are logical **dp**; conversion to px happens in layout.
#[derive(Clone, Debug)]
pub struct Config {
    pub height_dp: u32,            // default 35
    pub reduced_motion: bool,      // respect user pref
    pub anim_duration_ms: u32,     // slide duration per panel
    pub easing: Easing,

    /// Icon IDs (resolved by libnexus::THEME)
    pub icon_notifications: String,
    pub icon_control_center: String,

    /// Optional color overrides; None → resolve via THEME keys.
    pub bar_bg: Option<Color>,
    pub button_hover_veil: Option<Color>,
    pub panel_bg: Option<Color>,

    /// Panel widths in dp (clamped at runtime to screen)
    pub notifications_width_dp: u32,
    pub control_center_width_dp: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            height_dp: 35,
            reduced_motion: false,
            anim_duration_ms: 180,
            easing: Easing::CubicOut,

            icon_notifications: "notifications.button".into(),
            icon_control_center: "controlcenter.button".into(),

            bar_bg: None, // Will be resolved via THEME at runtime
            button_hover_veil: None, // Will be resolved via THEME at runtime
            panel_bg: None, // Will be resolved via THEME at runtime


            notifications_width_dp: 360,
            control_center_width_dp: 420,
        }
    }
}

// -------- UI THEME --------
// Theme paints (color + acrylic) loaded from nexus-assets via libnexus
// Fallback values match nexus.toml for consistency

// Action bar colors (no acrylic needed)
pub fn bar_bg_paint() -> Paint {
    THEME.paint("actionbar_bg", Paint {
        color: Color::rgba(255, 255, 255, 89),
        acrylic: None
    })
}

pub fn panel_bg_paint() -> Paint {
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
        color: Color::rgba(0, 0, 0, 26),
        acrylic: None
    })
}
