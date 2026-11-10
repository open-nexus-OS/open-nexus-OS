// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The colour vocabulary (RFC-0116): exactly the design artifacts' roles plus the two
//! design values they keep outside their token file (`WallpaperTint`, `LabelShadow`) and
//! `Transparent`. One enum; its canonical names are the theme keys and the DSL names, so the
//! theme build (`build.rs` ROLES, exhaustive over this enum), the DSL checker (`dsl/core`
//! COLOR_TOKENS) and the runtime map (`dsl/runtime` `color_token`) stay in lock-step
//! (`token_vocabulary_lockstep` enforces it).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: userspace/dsl/runtime/tests/token_vocabulary_lockstep.rs; the tests in lib.rs

/// Semantic colour roles — colours are roles, never swatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorToken {
    /// Opaque page area behind content that is not on the wallpaper.
    Background,
    /// Primary text and icons.
    Label,
    /// Metadata, section labels, inactive icons.
    LabelSecondary,
    /// Placeholders and disabled text only.
    LabelTertiary,
    /// Every hairline: dividers, list separators, pane and chip borders.
    Separator,
    /// The resting fill of a control on a surface.
    Fill,
    /// The hover layer (shown by opacity, never a background).
    FillHover,
    /// The press layer (shown by opacity, never a background).
    FillActive,
    /// A blur surface on the wallpaper: panels, dock, launcher sheets.
    GlassPanel,
    /// An app window's blur surface.
    GlassWindow,
    /// A translucent layer on a blur surface: cards, grouped rows, tiles.
    GlassCard,
    /// A floating surface's frosted glass — modal, alert, menu, popover, toast: dense enough to
    /// read on, over the regular blur (the design system's glass overlay).
    GlassOverlay,
    /// The 1 px light edge of every glass layer.
    GlassEdge,
    /// The opaque pane inside a window.
    SurfacePane,
    /// The opaque floating surface (the keyboard's tray; a floating surface where nothing may
    /// show through).
    SurfaceOverlay,
    /// The dimming layer behind a modal overlay.
    Scrim,
    /// The brand's solid button.
    Primary,
    /// Text and icons on `Primary`.
    OnPrimary,
    /// The one tint: on-states, selection, links, focus.
    Accent,
    /// The soft glow under an on-state control.
    AccentGlow,
    /// Text and icons on `Accent` and `Danger`.
    OnAccent,
    /// Destructive actions, errors, the notification dot.
    Danger,
    /// Success and online status.
    Success,
    /// Warning status.
    Warning,
    /// The airplane-mode toggle's on-state.
    ModeAirplane,
    /// The desktop-mode switch's on-state.
    ModeDesktop,
    /// The tablet-mode switch's on-state.
    ModeTablet,
    /// The activity runner's background.
    Island,
    /// Chart series 1–5.
    Chart1,
    Chart2,
    Chart3,
    Chart4,
    Chart5,
    /// The wash over the wallpaper that keeps content legible.
    WallpaperTint,
    /// The halo behind an icon label on the wallpaper.
    LabelShadow,
    /// No colour (a gradient stop, a hidden edge).
    Transparent,
}

impl ColorToken {
    /// Every role, in declaration order.
    pub const ALL: [ColorToken; 36] = [
        ColorToken::Background,
        ColorToken::Label,
        ColorToken::LabelSecondary,
        ColorToken::LabelTertiary,
        ColorToken::Separator,
        ColorToken::Fill,
        ColorToken::FillHover,
        ColorToken::FillActive,
        ColorToken::GlassPanel,
        ColorToken::GlassWindow,
        ColorToken::GlassCard,
        ColorToken::GlassOverlay,
        ColorToken::GlassEdge,
        ColorToken::SurfacePane,
        ColorToken::SurfaceOverlay,
        ColorToken::Scrim,
        ColorToken::Primary,
        ColorToken::OnPrimary,
        ColorToken::Accent,
        ColorToken::AccentGlow,
        ColorToken::OnAccent,
        ColorToken::Danger,
        ColorToken::Success,
        ColorToken::Warning,
        ColorToken::ModeAirplane,
        ColorToken::ModeDesktop,
        ColorToken::ModeTablet,
        ColorToken::Island,
        ColorToken::Chart1,
        ColorToken::Chart2,
        ColorToken::Chart3,
        ColorToken::Chart4,
        ColorToken::Chart5,
        ColorToken::WallpaperTint,
        ColorToken::LabelShadow,
        ColorToken::Transparent,
    ];

    /// The canonical name: the theme key and the DSL name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ColorToken::Background => "background",
            ColorToken::Label => "label",
            ColorToken::LabelSecondary => "labelSecondary",
            ColorToken::LabelTertiary => "labelTertiary",
            ColorToken::Separator => "separator",
            ColorToken::Fill => "fill",
            ColorToken::FillHover => "fillHover",
            ColorToken::FillActive => "fillActive",
            ColorToken::GlassPanel => "glassPanel",
            ColorToken::GlassWindow => "glassWindow",
            ColorToken::GlassCard => "glassCard",
            ColorToken::GlassOverlay => "glassOverlay",
            ColorToken::GlassEdge => "glassEdge",
            ColorToken::SurfacePane => "surfacePane",
            ColorToken::SurfaceOverlay => "surfaceOverlay",
            ColorToken::Scrim => "scrim",
            ColorToken::Primary => "primary",
            ColorToken::OnPrimary => "onPrimary",
            ColorToken::Accent => "accent",
            ColorToken::AccentGlow => "accentGlow",
            ColorToken::OnAccent => "onAccent",
            ColorToken::Danger => "danger",
            ColorToken::Success => "success",
            ColorToken::Warning => "warning",
            ColorToken::ModeAirplane => "modeAirplane",
            ColorToken::ModeDesktop => "modeDesktop",
            ColorToken::ModeTablet => "modeTablet",
            ColorToken::Island => "island",
            ColorToken::Chart1 => "chart1",
            ColorToken::Chart2 => "chart2",
            ColorToken::Chart3 => "chart3",
            ColorToken::Chart4 => "chart4",
            ColorToken::Chart5 => "chart5",
            ColorToken::WallpaperTint => "wallpaperTint",
            ColorToken::LabelShadow => "labelShadow",
            ColorToken::Transparent => "transparent",
        }
    }

    /// The role a canonical name names (`None` for anything else).
    #[must_use]
    pub fn from_name(name: &str) -> Option<ColorToken> {
        ColorToken::ALL.into_iter().find(|t| t.name() == name)
    }
}
