// config/colors.rs - Theme colors and paint functions

use orbclient::Color;
use libnexus::themes::{THEME, Paint, Acrylic};
use orbfont::Font;

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

pub fn text_inverse_fg() -> Color {
    THEME.paint("text_inverse_fg", Paint {
        color: Color::rgba(0xFF, 0xFF, 0xFF, 255),
        acrylic: None
    }).color
}

pub fn text_fg() -> Color {
    THEME.paint("text_fg", Paint {
        color: Color::rgba(0x0A, 0x0A, 0x0A, 255),
        acrylic: None
    }).color
}

// -------- FONT LOADING --------
/// Load font with fallback strategy to avoid font loading errors
pub fn load_crisp_font() -> Font {
    // Try explicit SemiBold weight first, fallback to Regular, then default
    Font::find(Some("Sans"), Some("SemiBold"), None)
        .or_else(|_| Font::find(Some("Sans"), Some("Regular"), None))
        .or_else(|_| Font::find(Some("Sans"), None, None))
        .or_else(|_| Font::find(None, None, None))
        .unwrap_or_else(|_| Font::find(Some("Sans"), None, None).unwrap())
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
