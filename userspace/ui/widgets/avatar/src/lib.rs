// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Avatar` — the design artifacts' avatar (design `Avatar`, RFC-0116): an image, or initials
//! on a glass backing — `glassCard`, a 1 px `glassEdge` and the lit top edge — as a circle or a
//! rounded square (radius 8 up to 32 px, 12 up to 56, 16 above). The initials are semibold
//! `label`, +0.01 em, in the text style the box size picks (≤ 24 caption2 · ≤ 30 caption ·
//! ≤ 36 subheadline · ≤ 42 headline · ≤ 50 title3 · ≤ 64 title1 · larger largeTitle). A presence
//! status is a dot in the bottom-right corner — max(8, 0.26 × size), `success` / `danger` /
//! `warning` / `labelSecondary` — inside a 2 px `glassWindow` ring, never clipped by the tile.
//! A pure builder producing a `LayoutNode::Stack`. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, FxPx, Justify,
    LayoutNode, Material, Overflow, Position, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, RadiusToken, ShadowToken, TextStyleToken, Tokens};
use nexus_widget_text::Text;

/// Presence status (maps to a dot color).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarStatus {
    Online,
    Busy,
    Away,
    Offline,
}

impl AvatarStatus {
    /// The semantic color role for the presence dot.
    pub fn color(self) -> ColorToken {
        match self {
            AvatarStatus::Online => ColorToken::Success,
            AvatarStatus::Busy => ColorToken::Danger,
            AvatarStatus::Away => ColorToken::Warning,
            AvatarStatus::Offline => ColorToken::LabelSecondary,
        }
    }
}

/// Initials for a display name: the first letter of up to two whitespace-
/// separated words, uppercased. Empty input yields an empty string — the
/// avatar then renders as a bare backing, which is honest for "no user yet".
#[must_use]
pub fn initials_of(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// An avatar tile.
#[derive(Debug, Clone)]
pub struct Avatar {
    initials: Option<String>,
    image: Option<LayoutNode>,
    size: i32,
    square: bool,
    status: Option<AvatarStatus>,
    id: Option<&'static str>,
    material: Option<Material>,
    fg: Option<ColorToken>,
    type_size: Option<TextStyleToken>,
}

impl Default for Avatar {
    fn default() -> Self {
        Self {
            initials: None,
            image: None,
            size: 40,
            square: false,
            status: None,
            id: None,
            material: None,
            fg: None,
            type_size: None,
        }
    }
}

impl Avatar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Initials shown when there is no image.
    pub fn initials(mut self, initials: impl Into<String>) -> Self {
        self.initials = Some(initials.into());
        self
    }

    /// Initials DERIVED from a display name — the first letter of up to two
    /// words, uppercased ("Jenning Schäfer" → "JS", "Jenning" → "J").
    ///
    /// Deriving here rather than at the call site is what keeps every avatar
    /// in the OS consistent: the DSL has no string operations, so the
    /// alternative is each app inventing its own rule or the wire carrying a
    /// second field.
    pub fn name(mut self, name: &str) -> Self {
        self.initials = Some(initials_of(name));
        self
    }
    /// Image node (caller-decoded).
    pub fn image(mut self, image: LayoutNode) -> Self {
        self.image = Some(image);
        self
    }
    pub fn size(mut self, size: i32) -> Self {
        self.size = size.max(16);
        self
    }
    /// Rounded-square instead of circle.
    pub fn square(mut self, square: bool) -> Self {
        self.square = square;
        self
    }
    pub fn status(mut self, status: AvatarStatus) -> Self {
        self.status = Some(status);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The backing's material (default `Card`: `glassCard` + `glassEdge`); an avatar alone on
    /// the wallpaper takes a blurring one (`Thin`). The lit top edge stays either way.
    pub fn material(mut self, material: Material) -> Self {
        self.material = Some(material);
        self
    }

    /// Ink color for the initials (defaults to `Label`).
    pub fn fg(mut self, color: ColorToken) -> Self {
        self.fg = Some(color);
        self
    }

    /// Text style of the initials (default: the one [`initials_style`] picks for the size).
    pub fn type_size(mut self, token: TextStyleToken) -> Self {
        self.type_size = Some(token);
        self
    }

    /// The presence status the corner dot shows (`None` = no dot).
    pub fn presence(&self) -> Option<AvatarStatus> {
        self.status
    }

    /// The tile's corner radius: a circle, or a rounded square by size (8 · 12 · 16).
    #[must_use]
    pub fn radius(&self) -> FxPx {
        if !self.square {
            return FxPx::new(self.size / 2);
        }
        match self.size {
            ..=32 => RadiusToken::Sm.px(),
            33..=56 => RadiusToken::Md.px(),
            _ => RadiusToken::Lg.px(),
        }
    }

    /// The presence dot's diameter: 26 % of the avatar, never below 8 px.
    #[must_use]
    pub fn dot_size(&self) -> i32 {
        ((self.size * 26 + 50) / 100).max(8)
    }

    /// Build the avatar tile (image or initials on its backing), with its presence dot.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let radius = self.radius();
        let dot_size = self.dot_size();
        // The glass backing: the card recipe (or the caller's material) and the lit top edge.
        let mut visual =
            Style::new().material(self.material.unwrap_or(Material::Card), tokens).visual();
        visual.corner_radius = CornerRadius::uniform(radius);
        visual.inset_highlight = Some(tokens.shadow(ShadowToken::EdgeHighlight).color);

        let content = match (self.image, &self.initials) {
            (Some(image), _) => image,
            (None, Some(initials)) => {
                let style = self.type_size.unwrap_or_else(|| initials_style(self.size));
                let mut text = Text::new(initials.clone())
                    .text_style(style)
                    .weight(FontWeight::Semibold)
                    .color(self.fg.unwrap_or(ColorToken::Label))
                    .build(tokens);
                if let LayoutNode::Text(t, _) = &mut text {
                    t.style.tracking_milli_em = 10;
                }
                text
            }
            (None, None) => LayoutNode::Spacer(nexus_layout_types::Spacer {
                id: None,
                flex_grow: 0,
                min_size: None,
                item: FlexItem::default(),
            }),
        };

        let d = Some(FxPx::new(self.size));
        let square = |id: Option<&'static str>, overflow: Overflow| Stack {
            id,
            direction: Direction::Row,
            align: Align::Center,
            justify: Justify::Center,
            overflow,
            min_width: d,
            max_width: d,
            min_height: d,
            max_height: d,
            item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
            ..Stack::default()
        };
        let Some(status) = self.status else {
            return LayoutNode::Stack(
                square(self.id, Overflow::Hidden),
                visual,
                alloc::vec![content],
            );
        };
        // With a status the clipped tile sits in an unclipped box that also holds the dot.
        let tile = LayoutNode::Stack(square(None, Overflow::Hidden), visual, alloc::vec![content]);
        let dot = status_dot(tokens, status, self.size, dot_size);
        LayoutNode::Stack(
            square(self.id, Overflow::Visible),
            VisualStyle::default(),
            alloc::vec![tile, dot],
        )
    }
}

/// The initials' text style for an avatar of `size` px (about 40 % of the box, snapped to the
/// nearest style).
#[must_use]
pub fn initials_style(size: i32) -> TextStyleToken {
    match size {
        ..=24 => TextStyleToken::Caption2,
        25..=30 => TextStyleToken::Caption,
        31..=36 => TextStyleToken::Subheadline,
        37..=42 => TextStyleToken::Headline,
        43..=50 => TextStyleToken::Title3,
        51..=64 => TextStyleToken::Title1,
        _ => TextStyleToken::LargeTitle,
    }
}

/// The presence dot in the bottom-right corner of a `size` px avatar: the status colour inside
/// a 2 px `glassWindow` ring (the ring is not under the colour — the design's padding-box fill).
fn status_dot(tokens: &dyn Tokens, status: AvatarStatus, size: i32, dot: i32) -> LayoutNode {
    let round = CornerRadius::uniform(RadiusToken::Full.px());
    let inner = Some(FxPx::new(dot - 4));
    let fill = LayoutNode::Stack(
        Stack {
            min_width: inner,
            max_width: inner,
            min_height: inner,
            max_height: inner,
            ..Stack::default()
        },
        VisualStyle {
            background: Some(tokens.color(status.color())),
            corner_radius: round,
            ..VisualStyle::default()
        },
        alloc::vec![],
    );
    let outer = Some(FxPx::new(dot));
    let at = FxPx::new(size - dot);
    LayoutNode::Stack(
        Stack {
            padding: EdgeInsets::all(FxPx::new(2)),
            min_width: outer,
            max_width: outer,
            min_height: outer,
            max_height: outer,
            item: FlexItem {
                position: Position::Absolute,
                margin: EdgeInsets { top: at, left: at, ..EdgeInsets::zero() },
                ..FlexItem::default()
            },
            ..Stack::default()
        },
        VisualStyle {
            border: EdgeBorder::all(FxPx::new(2), tokens.color(ColorToken::GlassWindow)),
            corner_radius: round,
            ..VisualStyle::default()
        },
        alloc::vec![fill],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout::LayoutEngine;
    use nexus_text_baked::measure_text::BakedTextMeasure;
    use nexus_theme_tokens::{BaseTokens, DarkTokens};

    #[test]
    fn the_backing_is_glass_card_with_edge_and_lit_top() {
        let t = BaseTokens;
        match Avatar::new().initials("LK").size(48).build(&t) {
            LayoutNode::Stack(stack, v, children) => {
                assert_eq!(stack.min_width, Some(FxPx::new(48)));
                assert_eq!(v.corner_radius.top_left, FxPx::new(24), "circle = size/2");
                assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
                assert_eq!(v.material, Some(Material::Card));
                assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
                assert_eq!(v.inset_highlight, Some(t.shadow(ShadowToken::EdgeHighlight).color));
                assert_eq!(children.len(), 1, "the initials");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn initials_come_from_the_display_name() {
        assert_eq!(initials_of("Jenning Schäfer"), "JS");
        assert_eq!(initials_of("Jenning"), "J");
        assert_eq!(initials_of("Ada Byron Lovelace"), "AB", "at most two words");
        assert_eq!(initials_of("  spaced   out  "), "SO");
        assert_eq!(initials_of("ätna öl"), "ÄÖ", "uppercasing is not ASCII-only");
        assert_eq!(initials_of(""), "", "no user yet renders a bare backing");
    }

    #[test]
    fn the_initials_follow_the_box_size() {
        let t = BaseTokens;
        for (size, px) in [(24, 10), (30, 11), (36, 13), (40, 15), (48, 17), (64, 24), (96, 30)] {
            match Avatar::new().initials("AB").size(size).build(&t) {
                LayoutNode::Stack(_, _, c) => match &c[0] {
                    LayoutNode::Text(n, _) => {
                        assert_eq!(n.style.font_size, FxPx::new(px), "size {size}");
                        assert_eq!(n.style.font_weight, FontWeight::Semibold);
                        assert_eq!(n.style.tracking_milli_em, 10);
                        assert_eq!(n.style.color, t.color(ColorToken::Label));
                    }
                    _ => panic!(),
                },
                _ => panic!(),
            }
        }
        // An explicit text style still wins.
        match Avatar::new().initials("AB").size(40).type_size(TextStyleToken::Caption).build(&t) {
            LayoutNode::Stack(_, _, c) => match &c[0] {
                LayoutNode::Text(n, _) => assert_eq!(n.style.font_size, FxPx::new(11)),
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn a_square_rounds_by_size() {
        assert_eq!(Avatar::new().size(32).square(true).radius(), FxPx::new(8));
        assert_eq!(Avatar::new().size(48).square(true).radius(), FxPx::new(12));
        assert_eq!(Avatar::new().size(64).square(true).radius(), FxPx::new(16));
    }

    #[test]
    fn the_status_dot_sits_in_the_corner_inside_a_ring() {
        let t = BaseTokens;
        let node =
            Avatar::new().initials("AB").size(40).status(AvatarStatus::Online).id("me").build(&t);
        match &node {
            LayoutNode::Stack(s, v, kids) => {
                assert_eq!(s.id, Some("me"));
                assert_eq!(s.overflow, Overflow::Visible, "the dot is never clipped");
                assert_eq!(*v, VisualStyle::default());
                assert_eq!(kids.len(), 2, "tile + dot");
                match &kids[1] {
                    LayoutNode::Stack(ds, dv, fill) => {
                        assert_eq!(ds.item.position, Position::Absolute);
                        assert_eq!(ds.min_width, Some(FxPx::new(10)), "26 % of 40");
                        assert_eq!(
                            dv.border.top.map(|b| (b.width, b.color)),
                            Some((FxPx::new(2), t.color(ColorToken::GlassWindow)))
                        );
                        match &fill[0] {
                            LayoutNode::Stack(_, fv, _) => {
                                assert_eq!(fv.background, Some(t.color(ColorToken::Success)))
                            }
                            _ => panic!(),
                        }
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
        let all = LayoutEngine::new()
            .layout(&node, FxPx::new(40), &BakedTextMeasure)
            .expect("layout")
            .boxes;
        let dot = all
            .iter()
            .find(|b| b.visual.border.top.map(|x| x.width) == Some(FxPx::new(2)))
            .expect("dot");
        assert_eq!((dot.rect.x.0, dot.rect.y.0, dot.rect.width.0), (30, 30, 10));
        assert_eq!(Avatar::new().size(20).dot_size(), 8, "never below 8");
    }

    #[test]
    fn a_material_and_the_dark_theme_stay_on_tokens() {
        let d = DarkTokens;
        match Avatar::new().name("Jenning").material(Material::Card).build(&d) {
            LayoutNode::Stack(_, v, _) => {
                assert_eq!(v.background, Some(d.color(ColorToken::GlassCard)));
                assert_eq!(v.inset_highlight, Some(d.shadow(ShadowToken::EdgeHighlight).color));
            }
            _ => panic!(),
        }
        assert_eq!(AvatarStatus::Online.color(), ColorToken::Success);
        assert_eq!(Avatar::new().status(AvatarStatus::Busy).presence(), Some(AvatarStatus::Busy));
        assert_eq!(Avatar::new().presence(), None);
    }
}
