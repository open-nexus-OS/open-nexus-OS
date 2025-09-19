use core::sync::atomic::{AtomicU8, Ordering};
use orbclient::Color;
use libnexus::themes::{THEME, Paint, Acrylic};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode { Desktop = 0, Mobile = 1 }

static MODE: AtomicU8 = AtomicU8::new(Mode::Desktop as u8);

pub fn set_mode(mode: Mode) { MODE.store(mode as u8, Ordering::Relaxed); }
pub fn mode() -> Mode { if MODE.load(Ordering::Relaxed) == 1 { Mode::Mobile } else { Mode::Desktop } }

#[derive(Copy, Clone, Debug)]
pub struct StartMenuConfig {
    /// Whether the desktop menu starts in small (panel) or large (expanded) mode
    pub desktop_large: bool,
}

impl Default for StartMenuConfig {
    fn default() -> Self { Self { desktop_large: false } }
}

static DESKTOP_LARGE: AtomicU8 = AtomicU8::new(0);

pub fn set_desktop_large(enabled: bool) { DESKTOP_LARGE.store(if enabled {1} else {0}, Ordering::Relaxed); }
pub fn desktop_large() -> bool { DESKTOP_LARGE.load(Ordering::Relaxed) == 1 }

// -------- UI THEME --------
// Theme paints (color + acrylic) loaded from nexus-assets via libnexus
// Fallback values match colors.toml for consistency

// Bar colors (no acrylic needed)
pub fn bar_paint() -> Paint {
    THEME.paint("menu_bar_bg", Paint {
        color: Color::rgba(0xFF, 0xFF, 0xFF, 191),
        acrylic: None
    })
}

pub fn bar_highlight_paint() -> Paint {
    THEME.paint("menu_bar_icon_bg_hover", Paint {
        color: Color::rgba(0x00, 0x00, 0x00, 26),
        acrylic: None
    })
}

pub fn bar_activity_marker_paint() -> Paint {
    THEME.paint("menu_bar_icon_active", Paint {
        color: Color::rgba(0x00, 0x00, 0x00, 255),
        acrylic: None
    })
}

pub fn text_paint() -> Paint {
    THEME.paint("text_fg", Paint {
        color: Color::rgba(0x0A, 0x0A, 0x0A, 255),
        acrylic: None
    })
}

pub fn text_highlight_paint() -> Paint {
    THEME.paint("text_highlight_fg", Paint {
        color: Color::rgba(0x0A, 0x0A, 0x0A, 255),
        acrylic: None
    })
}

// Menu surface colors with acrylic effect
pub fn menu_surface_sm_paint() -> Paint {
    THEME.paint("menu_surface_sm_bg", Paint {
        color: Color::rgba(255, 255, 255, 128),
        acrylic: Some(Acrylic {
            downscale: 4,
            tint: Color::rgba(255, 255, 255, 0),
            noise_alpha: 0
        })
    })
}

pub fn menu_surface_lg_paint() -> Paint {
    THEME.paint("menu_surface_lg_bg", Paint {
        color: Color::rgba(0, 0, 0, 64),
        acrylic: Some(Acrylic {
            downscale: 4,
            tint: Color::rgba(0, 0, 0, 0),
            noise_alpha: 0
        })
    })
}

// -------- UI CONSTANTS --------
pub const BAR_HEIGHT: u32 = 54;      // bar height (original: 54px)
pub const ICON_SCALE: f32 = 0.685;   // 68.5% of the bar height for icons (37/54 = 0.685)
pub const ICON_SMALL_SCALE: f32 = 0.75; // 75% of the bar height for small icons
