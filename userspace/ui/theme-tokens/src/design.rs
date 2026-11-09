// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The design artifacts' scales and recipes as typed tokens (RFC-0116): the 13 text
//! styles, the control sizes, the radii, the semantic gaps and the spacing grid, the shadows,
//! the backdrops and the material recipes (which role fills a surface, whether it blurs, its
//! edge and its shadow). The values come from `resources/themes` through `build.rs`; the
//! recipes and the grid are the contract's rules, written here once.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests in lib.rs

use nexus_layout_types::{FontWeight, FxPx, LineHeight, Material, Rgba8, TextStyle};

use crate::ColorToken;

/// The 13 text styles: size, line height, weight, tracking and case set together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextStyleToken {
    /// The lock screen's time (display × 2: 128 px, tabular digits).
    Clock,
    Display,
    LargeTitle,
    Title1,
    Title2,
    Title3,
    Headline,
    Body,
    Subheadline,
    Footnote,
    Caption,
    Caption2,
    Overline,
}

/// One text style's values (px; weight on Inter's axis; tracking in milli-em).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStyleSpec {
    pub size: u16,
    pub line: u16,
    pub weight: FontWeight,
    pub tracking_milli_em: i16,
    /// The style sets its text in upper case (`Overline`).
    pub upper: bool,
    /// The style's digits are tabular (one width): `Clock`.
    pub tabular: bool,
}

impl TextStyleToken {
    /// Every style, largest first.
    pub const ALL: [TextStyleToken; 13] = [
        TextStyleToken::Clock,
        TextStyleToken::Display,
        TextStyleToken::LargeTitle,
        TextStyleToken::Title1,
        TextStyleToken::Title2,
        TextStyleToken::Title3,
        TextStyleToken::Headline,
        TextStyleToken::Body,
        TextStyleToken::Subheadline,
        TextStyleToken::Footnote,
        TextStyleToken::Caption,
        TextStyleToken::Caption2,
        TextStyleToken::Overline,
    ];

    /// The canonical (DSL) name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            TextStyleToken::Clock => "clock",
            TextStyleToken::Display => "display",
            TextStyleToken::LargeTitle => "largeTitle",
            TextStyleToken::Title1 => "title1",
            TextStyleToken::Title2 => "title2",
            TextStyleToken::Title3 => "title3",
            TextStyleToken::Headline => "headline",
            TextStyleToken::Body => "body",
            TextStyleToken::Subheadline => "subheadline",
            TextStyleToken::Footnote => "footnote",
            TextStyleToken::Caption => "caption",
            TextStyleToken::Caption2 => "caption2",
            TextStyleToken::Overline => "overline",
        }
    }

    /// The style a canonical name names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<TextStyleToken> {
        TextStyleToken::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The style's values (theme-invariant).
    #[must_use]
    pub fn spec(self) -> TextStyleSpec {
        crate::text_style_spec(self)
    }

    /// The layout text style this style sets in `color`, with an optional weight override (the
    /// one override the contract allows). The caller upper-cases the content of an `upper` style.
    #[must_use]
    pub fn text_style(self, color: Rgba8, weight: Option<FontWeight>) -> TextStyle {
        let s = self.spec();
        TextStyle {
            font_size: FxPx::new(i32::from(s.size)),
            font_weight: weight.unwrap_or(s.weight),
            line_height: LineHeight::Absolute(FxPx::new(i32::from(s.line))),
            color,
            tracking_milli_em: s.tracking_milli_em,
            tabular: s.tabular,
            ..TextStyle::default()
        }
    }
}

/// The five control sizes: one outer height per size, so a row of controls lines up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ControlSize {
    Xs,
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
}

impl ControlSize {
    /// The outer height (px).
    #[must_use]
    pub fn height(self) -> FxPx {
        crate::control_height_px(self)
    }

    /// The label's style and weight: caption · footnote · body medium · headline · title3.
    #[must_use]
    pub const fn label(self) -> (TextStyleToken, Option<FontWeight>) {
        match self {
            ControlSize::Xs => (TextStyleToken::Caption, None),
            ControlSize::Sm => (TextStyleToken::Footnote, None),
            ControlSize::Md => (TextStyleToken::Body, Some(FontWeight::Medium)),
            ControlSize::Lg => (TextStyleToken::Headline, None),
            ControlSize::Xl => (TextStyleToken::Title3, None),
        }
    }

    /// The radius of a value field of this size: `sm` for xs/sm, `md` for md/lg, `lg` for xl.
    #[must_use]
    pub const fn field_radius(self) -> RadiusToken {
        match self {
            ControlSize::Xs | ControlSize::Sm => RadiusToken::Sm,
            ControlSize::Md | ControlSize::Lg => RadiusToken::Md,
            ControlSize::Xl => RadiusToken::Lg,
        }
    }
}

/// The radii; containers are concentric (outer `Xl`, inner = parent − padding, never below `Sm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RadiusToken {
    Sm,
    Md,
    Lg,
    Xl,
    /// A capsule or a circle.
    Full,
}

impl RadiusToken {
    /// The radius in px (`Full` = 9999).
    #[must_use]
    pub fn px(self) -> FxPx {
        crate::radius_px(self)
    }
}

/// The radius of a container nested in one of radius `parent` with `padding`: concentric,
/// never below `sm`.
#[must_use]
pub fn concentric(parent: FxPx, padding: FxPx) -> FxPx {
    let inner = parent.0.saturating_sub(padding.0);
    FxPx::new(inner.max(RadiusToken::Sm.px().0))
}

/// The semantic gaps on the spacing grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpaceToken {
    /// Between inline items (buttons in a bar, chips): 8.
    GapInline,
    /// Between items of one group (cards, tiles): 12.
    GapGroup,
    /// Between sections of a form or a panel: 24.
    GapSection,
    /// Content inset from a window, pane or panel edge: 16.
    MarginLayout,
}

impl SpaceToken {
    #[must_use]
    pub fn px(self) -> FxPx {
        crate::space_px(self)
    }
}

/// The spacing grid in px: the 4-pt steps plus the 2 px hairline offset and the 6 px half step
/// (only between an icon and its label). Nothing else is a spacing.
pub const SPACING_GRID: [u16; 12] = [0, 2, 4, 6, 8, 12, 16, 20, 24, 32, 40, 48];

/// Whether `px` lies on the spacing grid.
#[must_use]
pub fn on_spacing_grid(px: u16) -> bool {
    SPACING_GRID.contains(&px)
}

/// The 1 px hairline every border and separator is drawn with.
pub const HAIRLINE: FxPx = FxPx::new(1);

/// The shadows: four elevations, the recessed well and the lit top edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShadowToken {
    /// Knobs and thumbs.
    Shadow1,
    /// App icons, chips, the FAB.
    Shadow2,
    /// Menus, popovers, toasts, the dock, the action bar.
    Shadow3,
    /// Panels, modals, alerts, sheets, windows.
    Shadow4,
    /// A recessed well or track (inside the box).
    Inset,
    /// The 1 px light top edge of a raised surface (inside the box).
    EdgeHighlight,
}

/// One shadow (px; `inset` draws inside the box).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowSpec {
    pub x: i16,
    pub y: i16,
    pub blur: u16,
    pub spread: i16,
    pub color: Rgba8,
    pub inset: bool,
}

/// The backdrop blurs: only the surface of a stack that sits on the wallpaper carries one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackdropToken {
    UltraThin,
    Thin,
    Regular,
    Thick,
}

/// One backdrop: blur radius (px), saturation (%), the compositor's downsample factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backdrop {
    pub blur: u16,
    pub saturation: u16,
    pub downsample: u16,
}

/// What a material is made of: its fill role, its backdrop, its edge and its shadow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialRecipe {
    pub fill: ColorToken,
    pub backdrop: Option<BackdropToken>,
    /// The 1 px border (`GlassEdge` on glass and floating layers).
    pub edge: Option<ColorToken>,
    pub shadow: Option<ShadowToken>,
    /// The lit top edge of a raised surface.
    pub edge_highlight: bool,
}

/// The recipe of a material (RFC-0116 and the artifacts' liquid-glass rules).
#[must_use]
pub const fn recipe(material: Material) -> MaterialRecipe {
    let (fill, backdrop, edge, shadow, edge_highlight) = match material {
        Material::Panel => (
            ColorToken::GlassPanel,
            Some(BackdropToken::Regular),
            true,
            Some(ShadowToken::Shadow4),
            true,
        ),
        Material::Window => (
            ColorToken::GlassWindow,
            Some(BackdropToken::Regular),
            true,
            Some(ShadowToken::Shadow4),
            true,
        ),
        Material::Dock => (
            ColorToken::GlassPanel,
            Some(BackdropToken::Thick),
            true,
            Some(ShadowToken::Shadow3),
            true,
        ),
        Material::Thin => (ColorToken::GlassPanel, Some(BackdropToken::Thin), true, None, false),
        Material::UltraThin => {
            (ColorToken::GlassPanel, Some(BackdropToken::UltraThin), true, None, false)
        }
        Material::Card => (ColorToken::GlassCard, None, true, None, false),
        Material::Pane => (ColorToken::SurfacePane, None, false, None, false),
        Material::Overlay => {
            (ColorToken::SurfaceOverlay, None, true, Some(ShadowToken::Shadow3), true)
        }
    };
    MaterialRecipe {
        fill,
        backdrop,
        edge: if edge { Some(ColorToken::GlassEdge) } else { None },
        shadow,
        edge_highlight,
    }
}
