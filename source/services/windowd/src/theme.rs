// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! ⚠ CLEANUP-MAP (docs/dev/ui/windowd-cleanup-map.md): KEEP as a thin consumer — reached: the
//! values come from the typed tokens, one value source for the compositor AND the apps.
//

//! CONTEXT: The compositor's theme (TASK-0072 Phase 9, RFC-0116). The compositor holds the
//! active [`ThemeMode`] and reads a colour from the typed tokens of that mode
//! (`nexus-theme-tokens`, generated from the same `resources/themes/*.nxtheme.toml` authoring
//! as every app's), so a light/dark switch is a mode swap + a full redraw — no rebuild, and no
//! second baked copy of the theme in windowd. Colours come out BGRA8888 (the framebuffer
//! order), so surface renderers can write them directly.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
use nexus_theme_tokens::{ColorToken, DarkTokens, LightTokens, SolidTokens, Tokens};

/// The user's light/dark preference (the `ui.theme.mode` settings key).
/// (Consumed by the os-lite compositor runtime only.)
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeMode {
    Dark,
    Light,
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
impl ThemeMode {
    /// The `ui.theme.mode` wire value.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
        }
    }

    /// Parse the `ui.theme.mode` value; unknown → `None` (caller keeps default).
    pub(crate) fn from_str(s: &str) -> Option<Self> {
        match s {
            "dark" => Some(ThemeMode::Dark),
            "light" => Some(ThemeMode::Light),
            _ => None,
        }
    }

    /// The colour of `role` under this mode, BGRA8888.
    pub(crate) fn bgra(self, role: ColorToken) -> [u8; 4] {
        let c = match self {
            ThemeMode::Dark => DarkTokens.color(role),
            ThemeMode::Light => LightTokens.color(role),
        };
        [c.b, c.g, c.r, c.a]
    }

    /// The colour of `role` under this mode with reduce transparency on (`ui.material = solid`):
    /// the solid theme's grey surfaces, the colour app-host paints the same role in.
    pub(crate) fn solid_bgra(self, role: ColorToken) -> [u8; 4] {
        static SOLID_DARK: SolidTokens = SolidTokens { base: &DarkTokens };
        static SOLID_LIGHT: SolidTokens = SolidTokens { base: &LightTokens };
        let c = match self {
            ThemeMode::Dark => SOLID_DARK.color(role),
            ThemeMode::Light => SOLID_LIGHT.color(role),
        };
        [c.b, c.g, c.r, c.a]
    }

    /// An app window's shape under this mode (RFC-0116: the `window` material's radius, shadow
    /// and backdrop).
    pub(crate) fn window_shape(self) -> crate::window_shape::WindowShape {
        match self {
            ThemeMode::Dark => crate::window_shape::window_shape(&DarkTokens),
            ThemeMode::Light => crate::window_shape::window_shape(&LightTokens),
        }
    }
}
