// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: An app window's SHAPE as the compositor draws it (RFC-0116, the design artifacts'
//! window): the `window` material's recipe — its corner radius (`xl`), its drop shadow
//! (`shadow4`, the theme's strength: the dark theme's is stronger) and its backdrop (`regular`:
//! blur 40, saturation 180 %) — read from the token set, never a literal of windowd's own, so the
//! compositor and every app agree on one window. Pure, host-tested; the frame composite
//! (`compositor::shell_window`) and the starting surface take it per frame from the active theme.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the unit tests below.

use nexus_layout_types::Material;
use nexus_theme_tokens::{recipe, BackdropToken, BaseTokens, RadiusToken, ShadowToken, Tokens};

/// The shape of an app window under one theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WindowShape {
    /// The corner radius (px).
    pub(crate) radius: u32,
    /// The drop shadow: its blur (px), its downward offset (px), its strength (0–255).
    pub(crate) shadow_blur: u32,
    pub(crate) shadow_offset_y: i32,
    pub(crate) shadow_alpha: u32,
    /// The backdrop behind the glass: its blur radius (px) and saturation (%).
    pub(crate) blur_radius: u32,
    pub(crate) saturation_percent: u32,
}

/// The `window` material's shadow and backdrop (its recipe), and the window radius.
fn parts() -> (ShadowToken, BackdropToken) {
    let r = recipe(Material::Window);
    (r.shadow.unwrap_or(ShadowToken::Shadow4), r.backdrop.unwrap_or(BackdropToken::Regular))
}

/// The window's shape under `tokens` (the active theme).
#[must_use]
pub(crate) fn window_shape(tokens: &dyn Tokens) -> WindowShape {
    let (shadow, backdrop) = parts();
    let s = tokens.shadow(shadow);
    let b = tokens.backdrop(backdrop);
    WindowShape {
        radius: RadiusToken::Xl.px().0.max(0) as u32,
        shadow_blur: u32::from(s.blur),
        shadow_offset_y: i32::from(s.y),
        shadow_alpha: u32::from(s.color.a),
        blur_radius: u32::from(b.blur),
        saturation_percent: u32::from(b.saturation),
    }
}

/// How far the window's shadow reaches past its frame (its blur and its offset) — the damage a
/// moved or closed window leaves around it. The shadow's geometry is the same in every theme
/// (a theme changes its strength), so one number serves all.
#[must_use]
pub(crate) fn halo_pad() -> u32 {
    let s = BaseTokens.shadow(parts().0);
    u32::from(s.blur) + u32::from(s.y.unsigned_abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::{DarkTokens, LightTokens};

    /// The design's window: radius 24, shadow4 (0 25 50) at 20 % in light, 60 % in dark, the
    /// regular backdrop (blur 40, saturation 180 %).
    #[test]
    fn the_window_is_the_window_material() {
        let light = window_shape(&LightTokens);
        assert_eq!(light.radius, 24);
        assert_eq!((light.shadow_blur, light.shadow_offset_y), (50, 25));
        assert_eq!(light.shadow_alpha, 0x33);
        assert_eq!((light.blur_radius, light.saturation_percent), (40, 180));
        let dark = window_shape(&DarkTokens);
        assert_eq!(dark.shadow_alpha, 0x99, "the dark theme's shadow is stronger");
        assert_eq!(
            (dark.radius, dark.shadow_blur, dark.shadow_offset_y, dark.blur_radius),
            (24, 50, 25, 40)
        );
    }

    /// The damage around a window covers its whole shadow (blur below the offset frame).
    #[test]
    fn the_halo_covers_the_shadow() {
        assert_eq!(halo_pad(), 75);
        let s = window_shape(&DarkTokens);
        assert!(halo_pad() >= s.shadow_blur + s.shadow_offset_y.unsigned_abs());
    }
}
