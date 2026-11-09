// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The field kit the text inputs share (design artifacts `field.js`): the per-size field
//! metrics, the validation state, the box surface (filled · glass · read-only), the focus outline
//! and the small parts every field draws — label, message, the clear disc and the line glyphs.
//! TextField, TextArea, SearchBar and PinField build from here, so one size draws one field.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; glass_text_field.rs, pin_field.rs; the search_bar and text_area crates
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, Fraction, FxPx,
    Justify, LayoutNode, PathPoint, PathShape, Position, Rgba8, ShapeKind, Stack, TextAlign,
    TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
};
use nexus_style::Style;
use nexus_theme_tokens::{
    ColorToken, ControlSize, RadiusToken, ShadowToken, TextStyleToken, Tokens, HAIRLINE,
};

pub use crate::glyphs::{CHECK, CROSS, EYE, EYE_OFF, MAGNIFIER, STROKE_2, STROKE_2_5, STROKE_3};

/// Opacity of a disabled text field, search bar, text area or progress bar (design: 0.5).
pub const DISABLED_OPACITY: Fraction = Fraction::new(128);

/// Width of the focus outline (design `bundle.css` §3: `outline: 2px solid`).
pub const RING_WIDTH: i32 = 2;
/// Gap between a box and its focus outline (`outline-offset: 2px`).
pub const RING_OFFSET: i32 = 2;

/// One size's field geometry (design `FIELD_METRICS`). The field text is one step larger than a
/// button label of the same size; the outer height is the control height, so a field lines up
/// with the buttons, segments and steppers of its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMetrics {
    /// The outer (border-box) height.
    pub height: FxPx,
    /// The typed text and the placeholder (set at weight 500).
    pub text: TextStyleToken,
    /// The label above the box (set at weight 600).
    pub label: TextStyleToken,
    /// Horizontal padding inside the box.
    pub pad: FxPx,
    /// Gap between the parts in the box.
    pub gap: FxPx,
    /// Icon edge length (px).
    pub icon: i32,
}

impl FieldMetrics {
    /// The metrics of `size`.
    #[must_use]
    pub fn of(size: ControlSize) -> Self {
        let (text, label, pad, gap, icon) = match size {
            ControlSize::Xs => (TextStyleToken::Footnote, TextStyleToken::Caption, 8, 4, 12),
            ControlSize::Sm => (TextStyleToken::Subheadline, TextStyleToken::Footnote, 12, 6, 14),
            ControlSize::Md => (TextStyleToken::Body, TextStyleToken::Footnote, 12, 8, 16),
            ControlSize::Lg => (TextStyleToken::Headline, TextStyleToken::Subheadline, 16, 8, 18),
            ControlSize::Xl => (TextStyleToken::Title3, TextStyleToken::Body, 16, 12, 20),
        };
        Self { height: size.height(), text, label, pad: FxPx::new(pad), gap: FxPx::new(gap), icon }
    }

    /// The clear disc's diameter: `round(icon × 1.1)`.
    #[must_use]
    pub const fn clear_disc(&self) -> i32 {
        (self.icon * 11 + 5) / 10
    }

    /// The reveal button's diameter: `icon + 8`.
    #[must_use]
    pub const fn reveal_button(&self) -> i32 {
        self.icon + 8
    }

    /// The field text's line height (px).
    #[must_use]
    pub fn line(&self) -> i32 {
        i32::from(self.text.spec().line)
    }
}

/// A field's validation state (design `fieldState`): an error message always wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldState {
    #[default]
    Default,
    Error,
    Success,
}

impl FieldState {
    /// The border that replaces the resting one (`None` at rest).
    #[must_use]
    pub const fn border(self) -> Option<ColorToken> {
        match self {
            FieldState::Default => None,
            FieldState::Error => Some(ColorToken::Danger),
            FieldState::Success => Some(ColorToken::Success),
        }
    }

    /// The focus outline: the tint, or the state colour while invalid / valid.
    #[must_use]
    pub const fn ring(self, tint: ColorToken) -> ColorToken {
        match self {
            FieldState::Default => tint,
            FieldState::Error => ColorToken::Danger,
            FieldState::Success => ColorToken::Success,
        }
    }

    /// The message line's colour (helper text at rest).
    #[must_use]
    pub const fn message(self) -> ColorToken {
        match self {
            FieldState::Default => ColorToken::LabelSecondary,
            FieldState::Error => ColorToken::Danger,
            FieldState::Success => ColorToken::Success,
        }
    }
}

/// The box surface (design `fieldSurface`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldAppearance {
    /// The resting control fill — the default.
    #[default]
    Filled,
    /// A field straight on the wallpaper or a blur surface (the lock screen): the glass-card
    /// layer with its edge, edge highlight and `shadow1`. Never blurs.
    Glass,
}

/// The box surface: fill, 1 px state border, and for glass the edge highlight and `shadow1`. A
/// read-only field has no fill and a `separator` hairline (glass: its edge, no shadow). At rest
/// the 1 px border is transparent, so it is not drawn — [`box_padding`] still reserves it.
#[must_use]
pub fn surface(
    appearance: FieldAppearance,
    state: FieldState,
    read_only: bool,
    tokens: &dyn Tokens,
) -> Style {
    let mut s = Style::new();
    let rest_edge = match appearance {
        FieldAppearance::Filled => {
            if !read_only {
                s = s.background_token(tokens, ColorToken::Fill);
            }
            read_only.then_some(ColorToken::Separator)
        }
        FieldAppearance::Glass => {
            if !read_only {
                s = s
                    .background_token(tokens, ColorToken::GlassCard)
                    .shadow_token(tokens, ShadowToken::EdgeHighlight)
                    .shadow_token(tokens, ShadowToken::Shadow1);
            }
            Some(ColorToken::GlassEdge)
        }
    };
    if let Some(edge) = state.border().or(rest_edge) {
        s = s.border_token(tokens, edge);
    }
    s
}

/// A field box's padding: `pad` on both sides plus the 1 px border the design's box always has
/// (border-box; our borders are paint-only, so the inset is reserved here).
#[must_use]
pub fn box_padding(pad: FxPx, vertical: FxPx) -> EdgeInsets {
    EdgeInsets::symmetric(vertical, pad + HAIRLINE)
}

/// The radius of a value field of `size`, or a capsule.
#[must_use]
pub fn field_radius(size: ControlSize, capsule: bool) -> FxPx {
    if capsule {
        RadiusToken::Full.px()
    } else {
        size.field_radius().px()
    }
}

/// The focus outline (design `bundle.css` §3): a `RING_WIDTH` line `offset` px outside the box,
/// following its radius, without touching layout. Emitted as an ABSOLUTE child of the box —
/// absolute children are never measured — whose inner ring is pulled past the box edge by
/// negative margins. `inset` is the box's padding (absolute children sit in the content box)
/// and `radius` the box's own radius.
#[must_use]
pub fn focus_ring(color: Rgba8, radius: FxPx, inset: EdgeInsets, offset: i32) -> LayoutNode {
    let out = offset + RING_WIDTH;
    let outer_radius =
        if radius >= RadiusToken::Full.px() { radius } else { radius + FxPx::new(out) };
    let ring = LayoutNode::Stack(
        Stack {
            item: FlexItem {
                flex_grow: 1,
                margin: EdgeInsets {
                    top: FxPx::new(-(inset.top.0 + out)),
                    bottom: FxPx::new(-(inset.bottom.0 + out)),
                    ..EdgeInsets::zero()
                },
                ..FlexItem::default()
            },
            ..Stack::default()
        },
        VisualStyle {
            border: EdgeBorder::all(FxPx::new(RING_WIDTH), color),
            corner_radius: CornerRadius::uniform(outer_radius),
            ..VisualStyle::default()
        },
        Vec::new(),
    );
    LayoutNode::Stack(
        Stack {
            direction: Direction::Column,
            align: Align::Stretch,
            item: FlexItem {
                position: Position::Absolute,
                flex_grow: 1,
                margin: EdgeInsets {
                    left: FxPx::new(-(inset.left.0 + out)),
                    right: FxPx::new(-(inset.right.0 + out)),
                    ..EdgeInsets::zero()
                },
                ..FlexItem::default()
            },
            ..Stack::default()
        },
        VisualStyle::default(),
        alloc::vec![ring],
    )
}

/// A one-line text node in `style` (no wrapping).
#[must_use]
pub fn text_node(content: impl Into<String>, style: TextStyle) -> LayoutNode {
    LayoutNode::Text(
        TextNode {
            content: TextContent::new(content.into()),
            style: TextStyle { white_space: WhiteSpace::NoWrap, ..style },
            max_lines: Some(1),
            ..TextNode::default()
        },
        VisualStyle::default(),
    )
}

/// The label above a field: the size's label style at weight 600, `labelSecondary`, inset 2 px.
#[must_use]
pub fn label_node(label: &str, metrics: &FieldMetrics, tokens: &dyn Tokens) -> LayoutNode {
    let style = metrics
        .label
        .text_style(tokens.color(ColorToken::LabelSecondary), Some(FontWeight::Semibold));
    inset_left(text_node(label, style), FxPx::new(2))
}

/// The message under a field: `footnote`, inset 2 px, in the state's colour.
#[must_use]
pub fn message_node(message: &str, state: FieldState, tokens: &dyn Tokens) -> LayoutNode {
    let style = TextStyleToken::Footnote.text_style(tokens.color(state.message()), None);
    inset_left(text_node(message, style), FxPx::new(2))
}

/// Shift a node right by `px` inside its column (the design's `padding-left: 2px` on a line).
fn inset_left(mut node: LayoutNode, px: FxPx) -> LayoutNode {
    if let LayoutNode::Text(text, _) = &mut node {
        text.item.margin.left = px;
    }
    node
}

/// The typed text's style: the field text at weight 500 in `label`.
#[must_use]
pub fn input_style(metrics: &FieldMetrics, tokens: &dyn Tokens) -> TextStyle {
    TextStyle {
        text_align: TextAlign::Left,
        white_space: WhiteSpace::NoWrap,
        ..metrics.text.text_style(tokens.color(ColorToken::Label), Some(FontWeight::Medium))
    }
}

/// A line glyph: normalized `0..1000` polylines stroked `width_milli` with round caps, in a
/// `size` px box, tinted `color`.
#[must_use]
pub fn glyph(paths: &[&[(u16, u16)]], width_milli: u16, size: i32, color: Rgba8) -> LayoutNode {
    let paths = paths
        .iter()
        .map(|p| PathShape {
            points: p.iter().map(|&(x, y)| PathPoint::new(x, y)).collect(),
            closed: false,
        })
        .collect();
    let d = Some(FxPx::new(size));
    LayoutNode::Stack(
        Stack {
            min_width: d,
            max_width: d,
            min_height: d,
            max_height: d,
            item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
            ..Stack::default()
        },
        VisualStyle {
            background: Some(color),
            shape: ShapeKind::Stroke { paths, width_milli },
            ..VisualStyle::default()
        },
        Vec::new(),
    )
}

/// The clear disc (design `ClearButton`): a `labelSecondary` circle with the ✕ at 55 % in the
/// `background` colour, stroke 3. `id` makes it a tap target (the app clears the value).
#[must_use]
pub fn clear_disc(size: i32, id: Option<&'static str>, tokens: &dyn Tokens) -> LayoutNode {
    let d = Some(FxPx::new(size));
    let mark = glyph(CROSS, STROKE_3, (size * 55 + 50) / 100, tokens.color(ColorToken::Background));
    LayoutNode::Stack(
        Stack {
            id,
            align: Align::Center,
            justify: Justify::Center,
            min_width: d,
            max_width: d,
            min_height: d,
            max_height: d,
            item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
            ..Stack::default()
        },
        VisualStyle {
            background: Some(tokens.color(ColorToken::LabelSecondary)),
            corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
            ..VisualStyle::default()
        },
        alloc::vec![mark],
    )
}

/// `color` at `percent` % of its own alpha — the design's tint wash (`color-mix(in srgb, c p%,
/// transparent)`): 16 fill, 30 border, 55 line.
#[must_use]
pub const fn wash(color: Rgba8, percent: u32) -> Rgba8 {
    Rgba8::new(color.r, color.g, color.b, ((color.a as u32 * percent + 50) / 100) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::{BaseTokens, DarkTokens};

    #[test]
    fn metrics_follow_the_design_table() {
        let md = FieldMetrics::of(ControlSize::Md);
        assert_eq!(
            (md.height, md.pad, md.gap, md.icon),
            (FxPx::new(36), FxPx::new(12), FxPx::new(8), 16)
        );
        assert_eq!((md.text, md.label), (TextStyleToken::Body, TextStyleToken::Footnote));
        let xl = FieldMetrics::of(ControlSize::Xl);
        assert_eq!(
            (xl.height, xl.pad, xl.gap, xl.text),
            (FxPx::new(52), FxPx::new(16), FxPx::new(12), TextStyleToken::Title3)
        );
        // round(icon × 1.1): 13 / 15 / 18 / 20 / 22.
        let discs: Vec<i32> =
            [ControlSize::Xs, ControlSize::Sm, ControlSize::Md, ControlSize::Lg, ControlSize::Xl]
                .iter()
                .map(|s| FieldMetrics::of(*s).clear_disc())
                .collect();
        assert_eq!(discs, alloc::vec![13, 15, 18, 20, 22]);
        assert_eq!(md.reveal_button(), 24);
    }

    #[test]
    fn a_filled_field_rests_on_the_fill_without_a_visible_edge() {
        let t = BaseTokens;
        let v = surface(FieldAppearance::Filled, FieldState::Default, false, &t).visual();
        assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
        assert_eq!(v.border, EdgeBorder::none());
        let ro = surface(FieldAppearance::Filled, FieldState::Default, true, &t).visual();
        assert_eq!(ro.background, None, "read-only has no fill");
        assert_eq!(ro.border.top.map(|b| b.color), Some(t.color(ColorToken::Separator)));
        let err = surface(FieldAppearance::Filled, FieldState::Error, false, &t).visual();
        assert_eq!(err.border.top.map(|b| b.color), Some(t.color(ColorToken::Danger)));
    }

    #[test]
    fn a_glass_field_is_the_card_layer_and_never_blurs() {
        let t = DarkTokens;
        let s = surface(FieldAppearance::Glass, FieldState::Default, false, &t);
        let v = s.visual();
        assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
        assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
        assert!(v.inset_highlight.is_some() && v.shadow.is_some());
        assert_eq!(s.backdrop_blur(), None);
        assert_eq!(v.material, None);
        let ok = surface(FieldAppearance::Glass, FieldState::Success, false, &t).visual();
        assert_eq!(ok.border.top.map(|b| b.color), Some(t.color(ColorToken::Success)));
    }

    #[test]
    fn the_focus_ring_hangs_outside_the_box_without_layout() {
        let inset = box_padding(FxPx::new(12), FxPx::ZERO);
        match focus_ring(Rgba8::WHITE, FxPx::new(12), inset, RING_OFFSET) {
            LayoutNode::Stack(outer, v, kids) => {
                assert_eq!(outer.item.position, Position::Absolute, "never measured");
                assert_eq!(outer.item.margin.left, FxPx::new(-(13 + 4)));
                assert_eq!(v, VisualStyle::default());
                match &kids[0] {
                    LayoutNode::Stack(ring, rv, _) => {
                        assert_eq!(ring.item.margin.top, FxPx::new(-4));
                        assert_eq!(rv.corner_radius.top_left, FxPx::new(16), "radius + 4");
                        assert_eq!(rv.border.top.map(|b| b.width), Some(FxPx::new(2)));
                        assert_eq!(rv.background, None);
                    }
                    _ => panic!("ring"),
                }
            }
            _ => panic!("outer"),
        }
    }

    #[test]
    fn every_glyph_stays_inside_the_viewbox() {
        for paths in [CROSS, CHECK, MAGNIFIER, EYE, EYE_OFF] {
            for p in paths {
                assert!(!p.is_empty());
                assert!(p.iter().all(|&(x, y)| x <= 1000 && y <= 1000));
            }
        }
    }

    #[test]
    fn wash_scales_the_role_alpha() {
        assert_eq!(wash(Rgba8::new(10, 20, 30, 255), 16).a, 41);
        assert_eq!(wash(Rgba8::new(10, 20, 30, 200), 30).a, 60);
    }

    #[test]
    fn the_state_picks_border_ring_and_message() {
        assert_eq!(FieldState::Default.ring(ColorToken::Accent), ColorToken::Accent);
        assert_eq!(FieldState::Error.ring(ColorToken::Accent), ColorToken::Danger);
        assert_eq!(FieldState::Success.message(), ColorToken::Success);
        assert_eq!(FieldState::Default.border(), None);
    }
}
