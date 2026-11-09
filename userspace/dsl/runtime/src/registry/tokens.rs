// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The DSL's token VOCABULARY: every `name -> value` map a modifier argument goes through
//! (RFC-0116: one token set).
//!
//! This is one half of the registry seam. It mirrors — and must stay in lock-step with —
//! `nexus_dsl_core::registry::TOKEN_VOCABULARIES`, which is what the CHECKER validates against:
//! the frontend rejects an unknown or retired name at compile time, and these maps turn the
//! accepted ones into values. Colour roles and text styles resolve through the typed tokens'
//! own name tables (`ColorToken::from_name`, `TextStyleToken::from_name`), so the theme crate
//! stays the one place a name is spelled.

use nexus_layout_types::{FxPx, Material};
use nexus_theme_tokens::{ColorToken, RadiusToken, ShadowToken, SpaceToken, TextStyleToken};

/// A spacing in px (RFC-0116: the 4-pt grid; the checker rejects an off-grid literal, so this
/// only bounds what an expression may evaluate to).
#[must_use]
pub fn spacing(px: i64) -> FxPx {
    FxPx::new(px.clamp(0, 1024) as i32)
}

/// A semantic spacing name: `gapInline` 8, `gapGroup` 12, `gapSection` 24, `marginLayout` 16.
#[must_use]
pub fn spacing_token(name: &str) -> Option<FxPx> {
    Some(
        match name {
            "gapInline" => SpaceToken::GapInline,
            "gapGroup" => SpaceToken::GapGroup,
            "gapSection" => SpaceToken::GapSection,
            "marginLayout" => SpaceToken::MarginLayout,
            _ => return None,
        }
        .px(),
    )
}

/// A colour role by its canonical name.
#[must_use]
pub fn color_token(name: &str) -> Option<ColorToken> {
    ColorToken::from_name(name)
}

/// A text style by its canonical name.
#[must_use]
pub fn text_style(name: &str) -> Option<TextStyleToken> {
    TextStyleToken::from_name(name)
}

/// The alpha of a tint wash strength (`.tint(role, level)`): `fill` 16 %, `border` 30 %,
/// `line` 55 % — the only alphas a role takes. Unknown = the default `fill`.
#[must_use]
pub fn tint_alpha(level: &str) -> u8 {
    match level {
        "border" => 77,
        "line" => 140,
        _ => 41,
    }
}

/// `.figures(tabular|proportional)`: whether a text's digits take one width.
#[must_use]
pub fn figures(name: &str) -> Option<bool> {
    match name {
        "tabular" => Some(true),
        "proportional" => Some(false),
        _ => None,
    }
}

/// What a `.shadow(…)` name draws: a shadow token of the theme, or the label halo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowName {
    /// The four elevations, the recessed well and the lit top edge.
    Token(ShadowToken),
    /// The `labelShadow` behind a text that sits on the wallpaper.
    Halo,
}

/// A `.shadow(…)` name.
#[must_use]
pub fn shadow(name: &str) -> Option<ShadowName> {
    Some(ShadowName::Token(match name {
        "shadow1" => ShadowToken::Shadow1,
        "shadow2" => ShadowToken::Shadow2,
        "shadow3" => ShadowToken::Shadow3,
        "shadow4" => ShadowToken::Shadow4,
        "shadowInset" => ShadowToken::Inset,
        "edgeHighlight" => ShadowToken::EdgeHighlight,
        "halo" => return Some(ShadowName::Halo),
        _ => return None,
    }))
}

/// Maps a `.fontWeight(<token>)` name to the weight; the atlas holds each style's faces and the
/// overrides the screens use (RFC-0116).
#[must_use]
pub fn font_weight(name: &str) -> Option<nexus_layout_types::FontWeight> {
    use nexus_layout_types::FontWeight;
    Some(match name {
        "light" => FontWeight::Light,
        "normal" => FontWeight::Normal,
        "medium" => FontWeight::Medium,
        "semibold" => FontWeight::Semibold,
        "bold" => FontWeight::Bold,
        _ => return None,
    })
}

/// Maps a `.textAlign(<token>)` name onto the layout enum.
#[must_use]
pub fn text_align(name: &str) -> Option<nexus_layout_types::TextAlign> {
    use nexus_layout_types::TextAlign;
    Some(match name {
        "left" => TextAlign::Left,
        "center" => TextAlign::Center,
        "right" => TextAlign::Right,
        _ => return None,
    })
}

/// Maps a `.border(<token>)` name onto a width: the 1 px `hairline` every border is drawn with,
/// and the 2 px `ring` of a selection or focus outline.
#[must_use]
pub fn border_width(name: &str) -> Option<FxPx> {
    Some(match name {
        "hairline" => nexus_theme_tokens::HAIRLINE,
        "ring" => FxPx::new(2),
        _ => return None,
    })
}

/// A radius token in px: `sm` 8, `md` 12, `lg` 16, `xl` 24, `full` (a capsule or a circle).
#[must_use]
pub fn radius(name: &str) -> Option<FxPx> {
    Some(
        match name {
            "sm" => RadiusToken::Sm,
            "md" => RadiusToken::Md,
            "lg" => RadiusToken::Lg,
            "xl" => RadiusToken::Xl,
            "full" => RadiusToken::Full,
            _ => return None,
        }
        .px(),
    )
}

/// A `.material(…)` name: the blur surfaces on the wallpaper, the translucent card and the two
/// opaque surfaces.
#[must_use]
pub fn material(name: &str) -> Option<Material> {
    Some(match name {
        "panel" => Material::Panel,
        "window" => Material::Window,
        "dock" => Material::Dock,
        "thin" => Material::Thin,
        "ultraThin" => Material::UltraThin,
        "card" => Material::Card,
        "pane" => Material::Pane,
        "overlay" => Material::Overlay,
        _ => return None,
    })
}
