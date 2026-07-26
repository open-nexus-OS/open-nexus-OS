// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The DSL's token VOCABULARY: every `name -> semantic token` map a modifier
//! argument goes through.
//!
//! This is one half of the registry seam. It mirrors — and must stay in
//! lock-step with — `nexus_dsl_core::registry::TOKEN_VOCABULARIES`, which is
//! what the CHECKER validates against: the frontend rejects an unknown token
//! at compile time, and these maps turn the accepted ones into values.

use nexus_layout_types::FxPx;
use nexus_theme_tokens::{ColorToken, Tokens, TypographyToken};

/// Spacing scale: one step = 4px (matches the theme spacing scale).
#[must_use]
pub fn spacing(step: i64) -> FxPx {
    FxPx::new((step.clamp(0, 256) * 4) as i32)
}

#[must_use]
pub fn color_token(name: &str) -> Option<ColorToken> {
    Some(match name {
        "surface" => ColorToken::Surface,
        "surfaceVariant" => ColorToken::SurfaceVariant,
        "onSurface" => ColorToken::OnSurface,
        "onSurfaceVariant" => ColorToken::OnSurfaceVariant,
        "accent" => ColorToken::Accent,
        "onAccent" => ColorToken::OnAccent,
        "border" => ColorToken::Border,
        "background" => ColorToken::Background,
        "islandBg" => ColorToken::IslandBg,
        "primary" => ColorToken::Primary,
        "onPrimary" => ColorToken::OnPrimary,
        "danger" => ColorToken::Danger,
        "onDanger" => ColorToken::OnDanger,
        "success" => ColorToken::Success,
        "onSuccess" => ColorToken::OnSuccess,
        "warning" => ColorToken::Warning,
        "onWarning" => ColorToken::OnWarning,
        "info" => ColorToken::Info,
        "onInfo" => ColorToken::OnInfo,
        "focusRing" => ColorToken::FocusRing,
        "shadow" => ColorToken::Shadow,
        "scrim" => ColorToken::Scrim,
        "destructive" => ColorToken::Destructive,
        "onDestructive" => ColorToken::OnDestructive,
        // RFC-0082 — content on glass / directly on the wallpaper.
        "onGlass" => ColorToken::OnGlass,
        "onGlassMuted" => ColorToken::OnGlassMuted,
        "onGlassStrong" => ColorToken::OnGlassStrong,
        "glassIcon" => ColorToken::GlassIcon,
        "glassPlaceholder" => ColorToken::GlassPlaceholder,
        "glassFocus" => ColorToken::GlassFocus,
        "glassFill" => ColorToken::GlassFill,
        "wallpaperTint" => ColorToken::WallpaperTint,
        "wallpaperVignette" => ColorToken::WallpaperVignette,
        "textShadow" => ColorToken::TextShadow,
        "textShadowStrong" => ColorToken::TextShadowStrong,
        "transparent" => ColorToken::Transparent,
        _ => return None,
    })
}

#[must_use]
pub fn type_size(name: &str) -> Option<TypographyToken> {
    Some(match name {
        "xs" => TypographyToken::Xs,
        "sm" => TypographyToken::Sm,
        "base" => TypographyToken::Base,
        "md" => TypographyToken::Md,
        "lg" => TypographyToken::Lg,
        "xl" => TypographyToken::Xl,
        "xxl" => TypographyToken::Xxl,
        "xxxl" => TypographyToken::Xxxl,
        "display" => TypographyToken::Display,
        "hero" => TypographyToken::Hero,
        _ => return None,
    })
}

/// Maps a `.shadow(<token>)` name to the design elevation scale.
#[must_use]
pub fn shadow_level(name: &str) -> Option<nexus_layout_types::ShadowLevel> {
    use nexus_layout_types::ShadowLevel;
    Some(match name {
        "sm" => ShadowLevel::Sm,
        "md" => ShadowLevel::Md,
        "lg" => ShadowLevel::Lg,
        "xl" => ShadowLevel::Xl,
        "xxl" => ShadowLevel::Xxl2,
        _ => return None,
    })
}

/// Maps a `.fontWeight(<token>)` name to the authoring weight. The baked
/// ladder serves Light/Regular/SemiBold; `medium` and `bold` are authoring
/// aliases that collapse onto those (RFC-0082).
#[must_use]
pub fn font_weight(name: &str) -> Option<nexus_layout_types::FontWeight> {
    use nexus_layout_types::FontWeight;
    Some(match name {
        "light" => FontWeight::Light,
        "regular" => FontWeight::Regular,
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

/// Maps a `.leading(<token>)` name onto the `[leading]` scale step.
#[must_use]
pub fn leading(name: &str) -> Option<nexus_theme_tokens::LeadingToken> {
    use nexus_theme_tokens::LeadingToken;
    Some(match name {
        "flat" => LeadingToken::Flat,
        "tight" => LeadingToken::Tight,
        "snug" => LeadingToken::Snug,
        "normal" => LeadingToken::Normal,
        "relaxed" => LeadingToken::Relaxed,
        _ => return None,
    })
}

/// The `.textShadow(...)` vocabulary (RFC-0082).
///
/// These are EMPHASIS steps, not blur radii. The row-based painter renders a
/// text shadow as one extra glyph pass at a 1px offset — there is no offscreen
/// buffer to blur in. `soft` separates large type from a busy photo; `strong`
/// is for small labels that would otherwise dissolve into it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextShadowStep {
    None,
    Soft,
    Strong,
}

impl TextShadowStep {
    /// The concrete shadow for this step under the active theme.
    pub fn resolve(self, tokens: &dyn Tokens) -> nexus_layout_types::TextShadow {
        let (role, dy) = match self {
            TextShadowStep::None => (ColorToken::Transparent, 0),
            TextShadowStep::Soft => (ColorToken::TextShadow, 1),
            TextShadowStep::Strong => (ColorToken::TextShadowStrong, 1),
        };
        nexus_layout_types::TextShadow {
            offset_x: FxPx::ZERO,
            offset_y: FxPx::new(dy),
            blur_radius: FxPx::ZERO,
            color: tokens.color(role),
        }
    }
}

/// Maps a `.textShadow(<token>)` name onto its emphasis step.
#[must_use]
pub fn text_shadow(name: &str) -> Option<TextShadowStep> {
    Some(match name {
        "none" => TextShadowStep::None,
        "soft" => TextShadowStep::Soft,
        "strong" => TextShadowStep::Strong,
        _ => return None,
    })
}

/// Maps a `.border(<token>)` length name onto a hairline width. The design
/// system has exactly one border width today; the token keeps the door open.
#[must_use]
pub fn border_width(name: &str) -> Option<FxPx> {
    Some(match name {
        "thin" | "hairline" => FxPx::new(1),
        "medium" => FxPx::new(2),
        "thick" => FxPx::new(3),
        _ => return None,
    })
}

#[must_use]
pub fn radius(name: &str) -> FxPx {
    // Design-handoff radius scale (reference/tokens/spacing.css): sm 6,
    // md 8, lg 10, xl 14, 2xl 16 — the previous 4/8/12/16 drifted from it.
    FxPx::new(match name {
        "sm" => 6,
        "md" => 8,
        "lg" => 10,
        "xl" => 14,
        "xxl" => 16,
        "full" => 9999,
        _ => 0,
    })
}
