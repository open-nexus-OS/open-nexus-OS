// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::fmt;

/// An RGBA8 color value with 8 bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorValue {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl ColorValue {
    /// Parse a hex color string.
    ///
    /// Accepted formats:
    /// - `#rrggbb` (alpha defaults to 0xFF)
    /// - `#rrggbbaa`
    /// - `#rgb` (expanded to `#rrggbb`)
    ///
    /// Returns an error for invalid hex digits or wrong length.
    pub fn from_hex(hex: &str) -> Result<Self, crate::error::ThemeError> {
        let hex = hex.strip_prefix('#').ok_or_else(|| crate::error::ThemeError::InvalidColor {
            value: hex.to_string(),
            reason: "missing '#' prefix".to_string(),
        })?;

        let (r, g, b, a) = match hex.len() {
            3 => {
                let r = Self::hex_digit(&hex[0..1])?;
                let g = Self::hex_digit(&hex[1..2])?;
                let b = Self::hex_digit(&hex[2..3])?;
                (r * 17, g * 17, b * 17, 255)
            }
            6 => {
                let r = Self::hex_byte(&hex[0..2])?;
                let g = Self::hex_byte(&hex[2..4])?;
                let b = Self::hex_byte(&hex[4..6])?;
                (r, g, b, 255)
            }
            8 => {
                let r = Self::hex_byte(&hex[0..2])?;
                let g = Self::hex_byte(&hex[2..4])?;
                let b = Self::hex_byte(&hex[4..6])?;
                let a = Self::hex_byte(&hex[6..8])?;
                (r, g, b, a)
            }
            _ => {
                return Err(crate::error::ThemeError::InvalidColor {
                    value: format!("#{hex}"),
                    reason: format!("expected 3, 6, or 8 hex digits, got {}", hex.len()),
                });
            }
        };

        Ok(ColorValue { r, g, b, a })
    }

    fn hex_digit(s: &str) -> Result<u8, crate::error::ThemeError> {
        let bytes = s.as_bytes();
        if bytes.len() != 1 {
            return Err(crate::error::ThemeError::InvalidColor {
                value: s.to_string(),
                reason: "expected single hex digit".to_string(),
            });
        }
        let b = bytes[0];
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            b'A'..=b'F' => Ok(b - b'A' + 10),
            _ => Err(crate::error::ThemeError::InvalidColor {
                value: s.to_string(),
                reason: "invalid hex digit".to_string(),
            }),
        }
    }

    fn hex_byte(s: &str) -> Result<u8, crate::error::ThemeError> {
        let hi = Self::hex_digit(&s[0..1])?;
        let lo = Self::hex_digit(&s[1..2])?;
        Ok(hi * 16 + lo)
    }
}

impl fmt::Display for ColorValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.a == 255 {
            write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            write!(f, "#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
}

/// A map of semantic token names to color values.
///
/// Example tokens: `accent`, `bg`, `fg`, `surface`, `border`, `muted`,
/// `danger`, `warning`, `success`, `focusRing`.
#[derive(Debug, Clone, Default)]
pub struct TokenMap {
    tokens: HashMap<String, ColorValue>,
}

impl TokenMap {
    pub fn new() -> Self {
        TokenMap { tokens: HashMap::new() }
    }

    pub fn insert(&mut self, name: String, color: ColorValue) {
        self.tokens.insert(name, color);
    }

    pub fn get(&self, name: &str) -> Option<&ColorValue> {
        self.tokens.get(name)
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ColorValue)> {
        self.tokens.iter()
    }
}

/// A named length scale (radii, spacing) in whole layout pixels. Theme-invariant
/// in practice (authored once in `base.nxtheme.toml`), but resolved through the
/// qualifier chain like tokens/materials so a variant *could* override it.
/// Example keys: `small`, `medium`, `large` (+ the finer handoff scale later).
#[derive(Debug, Clone, Default)]
pub struct ScaleMap {
    values: HashMap<String, u32>,
}

impl ScaleMap {
    pub fn new() -> Self {
        ScaleMap { values: HashMap::new() }
    }

    pub fn insert(&mut self, name: String, px: u32) {
        self.values.insert(name, px);
    }

    pub fn get(&self, name: &str) -> Option<u32> {
        self.values.get(name).copied()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &u32)> {
        self.values.iter()
    }
}

/// Material definition for UI surfaces.
#[derive(Debug, Clone)]
pub enum Material {
    Opaque,
    Glass(GlassMaterial),
}

/// Glass (frosted) material parameters.
#[derive(Debug, Clone)]
pub struct GlassMaterial {
    pub blur_radius_dp: u32,
    pub downsample_factor: u32,
    pub tint_color: ColorValue,
    pub tint_alpha: f32,
    pub edge_highlight_color: ColorValue,
    pub edge_highlight_alpha: f32,
    pub border_color: Option<ColorValue>,
    pub border_alpha: Option<f32>,
}
