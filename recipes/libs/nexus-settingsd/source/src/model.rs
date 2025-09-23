use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Desktop,
    Mobile,
}
impl Default for Mode { fn default() -> Self { Mode::Desktop } }

/// Theme mode for the whole shell.
/// Keep it simple for now: no “auto”. You can add `Auto` later if you like.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
}
impl Default for ThemeMode { fn default() -> Self { ThemeMode::Light } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub mode: Mode,
    pub theme_mode: ThemeMode,
    // Extendable:
    pub locale: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            mode: Mode::Desktop,
            theme_mode: ThemeMode::Light,
            locale: None,
        }
    }
}
