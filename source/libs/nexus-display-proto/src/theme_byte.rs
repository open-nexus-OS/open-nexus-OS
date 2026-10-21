// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The theme byte the presentation snapshot carries (RFC-0083 `OP_SURFACE_SETTINGS`'s
//! `theme` field): the mode in the low nibble, the accent-palette index in the high nibble, and
//! the solid-materials bit beside the mode (TASK-0337, reduce transparency). Split out of
//! `client_surface.rs` (the module-size ratchet) and re-exported there, so every user keeps its
//! path.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below

/// Theme modes (align with windowd `ThemeMode` + the DSL `LightTokens`/`DarkTokens`).
pub const THEME_LIGHT: u8 = 0;
pub const THEME_DARK: u8 = 1;
/// The theme byte packs `mode | accent << 4`: the LOW nibble is the mode
/// (`THEME_*`), the HIGH nibble the accent-palette index (0 = the theme's
/// built-in accent). Lives on inside the presentation snapshot's `theme`
/// field; split with these on decode.
pub const THEME_MODE_MASK: u8 = 0x0F;
pub const THEME_ACCENT_SHIFT: u8 = 4;
/// The SOLID-MATERIALS bit of the mode nibble (TASK-0337, `ui.material = solid`, "reduce
/// transparency"): every material is its tint at full opacity and nothing is blurred — a theme
/// variant like dark or light, so every surface re-resolves when it flips.
pub const THEME_SOLID: u8 = 0x08;
/// The mode itself (`THEME_*`) within the nibble.
const THEME_MODE_ONLY: u8 = 0x07;

/// Packs `mode` + accent-palette index into the theme byte.
#[must_use]
pub fn pack_theme(mode: u8, accent: u8) -> u8 {
    (mode & THEME_MODE_MASK) | (accent << THEME_ACCENT_SHIFT)
}

/// Splits the theme byte → `(mode, accent)`; the solid bit is [`theme_is_solid`]'s.
#[must_use]
pub fn unpack_theme(byte: u8) -> (u8, u8) {
    (byte & THEME_MODE_ONLY, byte >> THEME_ACCENT_SHIFT)
}

/// Whether the theme byte asks for solid materials (`THEME_SOLID`).
#[must_use]
pub const fn theme_is_solid(byte: u8) -> bool {
    byte & THEME_SOLID != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The solid-materials bit rides beside the mode and unpacks away from it (TASK-0337): a
    /// dark solid theme is dark to every mode match, solid to `theme_is_solid`.
    #[test]
    fn the_solid_bit_rides_beside_the_mode_and_unpacks_away() {
        let solid_dark = pack_theme(THEME_DARK, 3) | THEME_SOLID;
        assert_eq!(unpack_theme(solid_dark), (THEME_DARK, 3));
        assert!(theme_is_solid(solid_dark));
        assert!(!theme_is_solid(pack_theme(THEME_LIGHT, 5)));
        assert_eq!(unpack_theme(pack_theme(THEME_LIGHT, 0)), (THEME_LIGHT, 0));
    }
}
