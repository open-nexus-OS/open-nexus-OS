// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! ⚠ CLEANUP-MAP (korrigiert): KEEP als dünner Konsument — die Token-WERTE sind
//! build-generiert aus resources/themes/*.nxtheme.toml (Value-SSOT). Offen ist die
//! App-Seite: ui/theme-tokens hartcodiert Werte → aus denselben .nxtheme.toml generieren.
//

//! CONTEXT: Runtime theme tokens (TASK-0072 Phase 9). The two qualifier
//! snapshots (`Dark`/`Light`) are baked from the vendored `.nxtheme.toml`
//! authoring by `build.rs` (`assets::THEME_DARK`/`THEME_LIGHT`) as
//! [`ThemeTokens`] consts; the compositor holds the active [`ThemeMode`] and
//! reads its colors from the matching snapshot, so a light/dark switch is a
//! const swap + a full redraw — no rebuild. Colors are BGRA8888 (the
//! framebuffer order), so surface renderers can write them directly.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable

/// The user's light/dark preference (the `ui.theme.mode` settings key).
/// (Consumed by the os-lite compositor runtime; host builds only see the
/// baked const types, hence the scoped allows in this module.)
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

    /// The opposite mode — the Settings "Theme" row toggles between the two.
    pub(crate) fn toggled(self) -> Self {
        match self {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        }
    }
}

/// Replace the alpha of a BGRA color, keeping its RGB. The theme tokens carry a
/// solid RGB (the `.nxtheme.toml` vocabulary); a surface's frosted translucency
/// is a per-surface material property, so renderers take the token's *color* at
/// their own tuned alpha rather than the token's (opaque) alpha.
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub(crate) const fn with_alpha(mut c: [u8; 4], alpha: u8) -> [u8; 4] {
    c[3] = alpha;
    c
}

/// The BGR triple of a BGRA color — the recolor tint for a monochrome glyph
/// sprite (the sprite's own alpha stays the anti-aliased coverage).
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub(crate) const fn rgb3(c: [u8; 4]) -> [u8; 3] {
    [c[0], c[1], c[2]]
}

/// One baked theme snapshot: the semantic tokens the compositor's chrome uses,
/// in BGRA8888. Field names mirror the `.nxtheme.toml` token vocabulary —
/// the FULL vocabulary is baked by build.rs even where the chrome reads only a
/// subset today (declared token surface, not dead weight).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ThemeTokens {
    /// Primary surface / window glass body tint.
    pub surface: [u8; 4],
    /// Secondary surface (rows, alt fills).
    pub surface_alt: [u8; 4],
    /// Hairline borders / section frames.
    pub border: [u8; 4],
    /// Primary foreground (labels, titles).
    pub fg: [u8; 4],
    /// Muted foreground (placeholders, secondary text).
    pub muted_fg: [u8; 4],
    /// Accent (hover tints, active state, values).
    pub accent: [u8; 4],
    /// Foreground ON an accent fill.
    pub accent_fg: [u8; 4],
    /// Frosted-glass body tint (translucent).
    pub glass_tint: [u8; 4],
    /// Glass edge highlight.
    pub glass_edge: [u8; 4],
}
