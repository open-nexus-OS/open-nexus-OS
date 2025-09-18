// themes/colors.rs — color acrylic types, parsing.
use orbclient::Color;
use serde::Deserialize;
use std::collections::HashMap;

/// Acrylic (blur-ish) parameters you can attach to a color.
/// - downscale: how much to downscale before upscaling again (cheap blur approximation)
/// - tint: overlay color applied on top of blurred region
/// - noise_alpha: strength of optional noise overlay (0..=255)
#[derive(Clone, Copy, Debug)]
pub struct Acrylic {
    pub downscale: u8,
    pub tint: Color,
    pub noise_alpha: u8,
}

/// A named paint entry: a base color plus optional acrylic effect.
#[derive(Clone, Copy, Debug)]
pub struct Paint {
    pub color: Color,
    pub acrylic: Option<Acrylic>,
}

// ---------- Parsing structs for theme `colors.toml` ----------
// You can define colors in multiple ways:
//   [colors]
//   bar = [255,255,255,191]
//   text = "#E7E7E7"
//   bar_highlight = { rgba = [255,255,255,200], acrylic = { enabled = true, downscale = 4, tint = "#33FFFFFF", noise_alpha = 16 } }

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ColorEntry {
    Array(Vec<u8>),
    Hex(String),
    Table(ColorTableEntry),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ColorTableEntry {
    #[serde(default)]
    pub rgba: Option<Vec<u8>>,
    #[serde(default)]
    pub hex: Option<String>,
    #[serde(default)]
    pub acrylic: Option<AcrylicEntry>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AcrylicEntry {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub downscale: Option<u8>,
    #[serde(default)]
    pub tint: Option<String>,      // hex string
    #[serde(default)]
    pub noise_alpha: Option<u8>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DefaultsSection {
    #[serde(default)]
    pub acrylic: Option<AcrylicEntry>,
}

/// File model for `/ui/themes/<theme>/colors.toml`
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ThemeColorsToml {
    #[serde(default)]
    pub colors: HashMap<String, ColorEntry>,
    #[serde(default)]
    pub defaults: DefaultsSection,
}

// ---------- Helpers used by the manager ----------
pub fn to_color(v: &[u8]) -> Color {
    match v {
        [r, g, b, a] => Color::rgba(*r, *g, *b, *a),
        [r, g, b]    => Color::rgba(*r, *g, *b, 255),
        _            => Color::rgba(255, 255, 255, 255),
    }
}

pub fn hex_to_color(s: &str) -> Option<Color> {
    fn nyb(h: u8) -> u8 {
        match h {
            b'0'..=b'9' => h - b'0',
            b'a'..=b'f' => 10 + (h - b'a'),
            b'A'..=b'F' => 10 + (h - b'A'),
            _ => 0
        }
    }
    let t = s.trim();
    let t = t.strip_prefix('#').unwrap_or(t);
    let b = t.as_bytes();
    let (r, g, bl, a) = match b.len() {
        6 => ((nyb(b[0])<<4)|nyb(b[1]), (nyb(b[2])<<4)|nyb(b[3]), (nyb(b[4])<<4)|nyb(b[5]), 255),
        8 => ((nyb(b[0])<<4)|nyb(b[1]), (nyb(b[2])<<4)|nyb(b[3]), (nyb(b[4])<<4)|nyb(b[5]), (nyb(b[6])<<4)|nyb(b[7])),
        _ => return None,
    };
    Some(Color::rgba(r, g, bl, a))
}
