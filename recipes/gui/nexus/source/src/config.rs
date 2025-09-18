use log::{debug, error};
use orbclient::{Color, Renderer};
use serde_derive::Deserialize;
use std::fs::File;
use std::io::Read;

// THEME-Adapter (libnexus → CoreImage)
use libnexus::{THEME, IconVariant};
use libnexus::{Acrylic, Paint};
use crate::core::image::Image as CoreImage;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
pub struct ConfigColor {
    data: u32,
}

impl From<ConfigColor> for Color {
    fn from(value: ConfigColor) -> Self {
        Self { data: value.data }
    }
}
impl From<Color> for ConfigColor {
    fn from(value: Color) -> Self {
        Self { data: value.data }
    }
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub cursor: String,
    pub bottom_left_corner: String,
    pub bottom_right_corner: String,
    pub bottom_side: String,
    pub left_side: String,
    pub right_side: String,
    pub window_max: String,
    pub window_max_unfocused: String,
    pub window_close: String,
    pub window_close_unfocused: String,

    #[serde(default = "background_color_default")]
    pub background_color: ConfigColor,
    #[serde(default = "bar_color_default")]
    pub bar_color: ConfigColor,
    #[serde(default = "bar_highlight_color_default")]
    pub bar_highlight_color: ConfigColor,
    #[serde(default = "text_color_default")]
    pub text_color: ConfigColor,
    #[serde(default = "text_highlight_color_default")]
    pub text_highlight_color: ConfigColor,
}

fn background_color_default() -> ConfigColor {
    // fallback for "window_bg"
    Color::rgba(2, 168, 38, 1).into()
}
fn bar_color_default() -> ConfigColor {
    // fallback for "window_titlebar_bg"
    Color::rgba(192, 33, 152, 1).into()
}
fn bar_highlight_color_default() -> ConfigColor {
    // fallback for "window_content_bg"
    Color::rgba(0x36, 0x36, 0x36, 224).into()
}
fn text_color_default() -> ConfigColor {
    // fallback for "text_fg"
    Color::rgb(0xE7, 0xE7, 0xE7).into()
}
fn text_highlight_color_default() -> ConfigColor {
    // fallback for "text_highlight_fg"
    Color::rgb(0xE7, 0xE7, 0xE7).into()
}

/// Create a sane default Orbital [Config] in case none is supplied or it is unreadable
impl Default for Config {
    fn default() -> Self {
        // Cannot use "..Default::default() for all these fields as that is recursive, so they
        // all have to be "defaulted" manually.
        Config {
            // TODO: What would be good or better defaults for these config values?
            cursor: String::default(),
            bottom_left_corner: String::default(),
            bottom_right_corner: String::default(),
            bottom_side: String::default(),
            left_side: String::default(),
            right_side: String::default(),
            window_max: String::default(),
            window_max_unfocused: String::default(),
            window_close: String::default(),
            window_close_unfocused: String::default(),

            // These are the default colors for Orbital that have been defined
            background_color: background_color_default(),
            bar_color: bar_color_default(),
            bar_highlight_color: bar_highlight_color_default(),
            text_color: text_color_default(),
            text_highlight_color: text_highlight_color_default(),
        }
    }
}

/// Small Wrapper used by UI code to hold a color + optional acrylic effect
#[derive(Clone, Copy, Debug)]
pub struct UiPaint {
    pub color: Color,
    pub acrylic: Option<Acrylic>,
}

/// [Config] holds configuration information for Orbital, such as colors, cursors etc.
impl Config {
    // returns the default config if the string passed is not a valid config
    fn config_from_string(config: &str) -> Config {
        match toml::from_str(config) {
            Ok(config) => config,
            Err(err) => {
                error!("failed to parse config '{}'", err);
                Config::default()
            }
        }
    }

    /// Read an Orbital configuration from a toml file at `path`
    pub fn from_path(path: &str) -> Config {
        let mut string = String::new();

        match File::open(path) {
            Ok(mut file) => match file.read_to_string(&mut string) {
                Ok(_) => debug!("reading config from path: '{}'", path),
                Err(err) => error!("failed to read config '{}': {}", path, err),
            },
            Err(err) => error!("failed to open config '{}': {}", path, err),
        }

        Self::config_from_string(&string)

    }

    // --- THEME helpers (icons/colors/acrylic) ----------------------------------------
    // Render a themed icon (key from nexus.toml) at an exact pixel size.
    pub fn themed_icon_px(&self, id: &str, px: u32) -> CoreImage {
        THEME
            .load_icon_sized(id, IconVariant::Auto, Some((px, px)))
            .map(|svg| {
                let w = svg.width() as i32;
                let h = svg.height() as i32;
                CoreImage::from_data(w, h, svg.data().to_vec().into())
            })
            .unwrap_or_else(|| CoreImage::new(0, 0))
    }

    // Convenience: scale-aware icon rendered from a logical base size.
    pub fn themed_icon_scaled(&self, id: &str, scale: i32, base_px: u32) -> CoreImage {
        let px = (base_px * scale.max(1) as u32).max(1);
        self.themed_icon_px(id, px)
    }

    // --- Window colors via libnexus (flat Color) -----------------------------
    // These keep the old function names but fetch by your new theme keys.
    // Acrylic is ignored here on purpose; use the *paint_* getters below when needed.
    pub fn color_background(&self) -> Color {
        // maps to "window_bg"
        let p = THEME.paint("window_bg", Paint { color: self.background_color.into(), acrylic: None });
        debug!("color_background: using theme color {:?} (fallback: {:?})", p.color, Color::from(self.background_color));
        p.color
    }
    pub fn color_bar(&self) -> Color {
        // maps to "window_titlebar_bg"
        let p = THEME.paint("window_titlebar_bg", Paint { color: self.bar_color.into(), acrylic: None });
        debug!("color_bar: using theme color {:?} (fallback: {:?})", p.color, Color::from(self.bar_color));
        p.color
    }
    pub fn color_bar_highlight(&self) -> Color {
        // maps to "window_content_bg"
        let p = THEME.paint("window_content_bg", Paint { color: self.bar_highlight_color.into(), acrylic: None });
        debug!("color_bar_highlight: using theme color {:?} (fallback: {:?})", p.color, Color::from(self.bar_highlight_color));
        p.color
    }
    pub fn color_text(&self) -> Color {
        // maps to "text_fg"
        let p = THEME.paint("text_fg", Paint { color: self.text_color.into(), acrylic: None });
        debug!("color_text: using theme color {:?} (fallback: {:?})", p.color, Color::from(self.text_color));
        p.color
    }
    pub fn color_text_highlight(&self) -> Color {
        // maps to "text_highlight_fg"
        let p = THEME.paint("text_highlight_fg", Paint { color: self.text_highlight_color.into(), acrylic: None });
        debug!("color_text_highlight: using theme color {:?} (fallback: {:?})", p.color, Color::from(self.text_highlight_color));
        p.color
    }

    // --- Optional: Window paints (Color + Acrylic) for window.rs -------------
    // Use these if you want to apply acrylic in the titlebar/content rendering.
    pub fn paint_window_bg(&self) -> (Color, Option<Acrylic>) {
        let p = THEME.paint("window_bg", Paint { color: self.background_color.into(), acrylic: None });
        (p.color, p.acrylic)
    }
    pub fn paint_window_titlebar_bg(&self) -> (Color, Option<Acrylic>) {
        let p = THEME.paint("window_titlebar_bg", Paint { color: self.bar_color.into(), acrylic: None });
        (p.color, p.acrylic)
    }
    pub fn paint_window_content_bg(&self) -> (Color, Option<Acrylic>) {
        let p = THEME.paint("window_content_bg", Paint { color: self.bar_highlight_color.into(), acrylic: None });
        (p.color, p.acrylic)
    }
    pub fn paint_text_fg(&self) -> (Color, Option<Acrylic>) {
        let p = THEME.paint("text_fg", Paint { color: self.text_color.into(), acrylic: None });
        (p.color, p.acrylic)
    }
    pub fn paint_text_highlight_fg(&self) -> (Color, Option<Acrylic>) {
        let p = THEME.paint("text_highlight_fg", Paint { color: self.text_highlight_color.into(), acrylic: None });
        (p.color, p.acrylic)
    }

    pub fn paint_shadow_color(&self) -> (Color, Option<Acrylic>) {
        let p = THEME.paint("shadow_color", Paint { color: Color::rgba(0, 0, 0, 64), acrylic: None });
        (p.color, p.acrylic)
    }

    // NEW: Paint getters (color + optional acrylic) fed from colors.toml
    // Use these in window/scheme drawing; switch on `paint.acrylic.is_some()`
    // to enable your acrylic/blur path.
    fn themed_paint(&self, key: &str, fallback: Color) -> UiPaint {
        let fb = Paint { color: fallback, acrylic: None };
        let p  = THEME.paint(key, fb);
        UiPaint { color: p.color, acrylic: p.acrylic }
    }

}

#[cfg(test)]
mod test {
    use crate::config::{background_color_default, text_highlight_color_default, Config};

    #[test]
    fn non_existent_config_file() {
        let config = Config::from_path("no-such-file.toml");
        assert_eq!(config.cursor, "");
        assert_eq!(config.text_highlight_color, text_highlight_color_default());
    }

    #[test]
    fn partial_config() {
        let config_str = r##"
            background_color = "#FFFFFFFF"
        "##;
        let config = Config::config_from_string(config_str);
        assert_eq!(config.background_color, background_color_default());
    }

    #[test]
    fn valid_partial_config() {
        let config_str = r##"cursor = "/ui/icons/cursor/left_ptr.png"
        bottom_left_corner = "/ui/icons/cursor/bottom_left_corner.png"
        bottom_right_corner = "/ui/icons/cursor/bottom_right_corner.png"
        bottom_side = "/ui/icons/cursor/bottom_side.png"
        left_side = "/ui/icons/cursor/left_side.png"
        right_side = "/ui/icons/cursor/right_side.png"
        window_max = "/ui/icons/actions/window_max.png"
        window_max_unfocused = "/ui/icons/actions/window_max_unfocused.png"
        window_close = "/ui/icons/actions/window_close.png"
        window_close_unfocused = "/ui/icons/actions/window_close_unfocused.png""##;
        let config = Config::config_from_string(config_str);
        assert_eq!(config.background_color, background_color_default());
        assert_eq!(config.bottom_left_corner, "/ui/bottom_left_corner.png");
    }
}
