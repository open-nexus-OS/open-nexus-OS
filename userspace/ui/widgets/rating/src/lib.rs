// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Rating` — the design-system star rating (the design artifacts' `Rating`): a row (gap 2) of
//! `max` stars (default 5) of `size` px (default 22), drawn with the artifacts' star path. An
//! empty star is a `fill` body with a 1.2 `separator` outline; the filled overlay is the same
//! star in `warning` (or the colour given), clipped to the star's share of the value — so a
//! read-only rating shows fractions (`value_tenths`).
//!
//! A pure builder producing a `LayoutNode::Stack` row; the `id` is the interaction id (the app
//! maps a tapped star index → value; a hover preview is a content change the app makes).
//! DSL-emittable.

extern crate alloc;

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, FlexItem, FxPx, Justify, LayoutNode, Overflow, Position, Stack, VisualStyle,
};
use nexus_style::glyph::{self, GridPoint};
use nexus_theme_tokens::{ColorToken, Tokens};

/// The star (`M12 2.5l2.9 6 6.6.9-4.8 4.6 1.2 6.5L12 18.9 6.1 21.5l1.2-6.5L2.5 9.4l6.6-.9z`).
const STAR: [GridPoint; 10] = [
    (120, 25),
    (149, 85),
    (215, 94),
    (167, 140),
    (179, 205),
    (120, 189),
    (61, 215),
    (73, 150),
    (25, 94),
    (91, 85),
];
/// The outline's stroke (1.2 grid units).
const OUTLINE: u16 = 12;

/// A star rating.
#[derive(Debug, Clone)]
pub struct Rating {
    /// The value in tenths of a star.
    tenths: u16,
    max: u8,
    size: i32,
    color: Option<ColorToken>,
    id: Option<&'static str>,
}

impl Default for Rating {
    fn default() -> Self {
        Self { tenths: 0, max: 5, size: 22, color: None, id: None }
    }
}

impl Rating {
    pub fn new() -> Self {
        Self::default()
    }

    /// Filled stars (clamped to `max`).
    pub fn value(mut self, value: u8) -> Self {
        self.tenths = u16::from(value) * 10;
        self
    }

    /// The value in tenths of a star (35 = 3.5 stars) — a read-only rating's fractions.
    pub fn value_tenths(mut self, tenths: u16) -> Self {
        self.tenths = tenths;
        self
    }

    pub fn max(mut self, max: u8) -> Self {
        self.max = max.max(1);
        self
    }

    /// The star size in px (default 22).
    pub fn size(mut self, px: i32) -> Self {
        self.size = px.max(8);
        self
    }

    /// The filled stars' colour role (default `warning`).
    pub fn color(mut self, color: ColorToken) -> Self {
        self.color = Some(color);
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// One star: the empty body and outline, and the filled overlay clipped to `ratio` tenths.
    fn star(&self, tokens: &dyn Tokens, ratio: u16) -> LayoutNode {
        let size = FxPx::new(self.size);
        let fill = tokens.color(self.color.unwrap_or(ColorToken::Warning));
        let overlay = |node: LayoutNode| match node {
            LayoutNode::Stack(mut stack, visual, children) => {
                stack.item.position = Position::Absolute;
                LayoutNode::Stack(stack, visual, children)
            }
            other => other,
        };
        let mut parts = alloc::vec![
            glyph::fill(&STAR, size, tokens.color(ColorToken::Fill)),
            overlay(glyph::stroke(
                &[&STAR],
                true,
                OUTLINE,
                size,
                tokens.color(ColorToken::Separator)
            )),
        ];
        if ratio > 0 {
            // The filled star, clipped to its share: a clip box `ratio` tenths of the star wide
            // holding a full-size star (it keeps its size; the box cuts it).
            let width = FxPx::new((self.size * i32::from(ratio.min(10)) + 5) / 10);
            let filled = alloc::vec![
                glyph::fill(&STAR, size, fill),
                overlay(glyph::stroke(&[&STAR], true, OUTLINE, size, fill)),
            ];
            parts.push(LayoutNode::Stack(
                Stack {
                    direction: Direction::Row,
                    overflow: Overflow::Hidden,
                    min_width: Some(width),
                    max_width: Some(width),
                    min_height: Some(size),
                    max_height: Some(size),
                    item: FlexItem { position: Position::Absolute, ..FlexItem::default() },
                    ..Stack::default()
                },
                VisualStyle::default(),
                filled,
            ));
        }
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                min_width: Some(size),
                max_width: Some(size),
                min_height: Some(size),
                max_height: Some(size),
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle::default(),
            parts,
        )
    }

    /// Build the rating row.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let shown = self.tenths.min(u16::from(self.max) * 10);
        let stars: Vec<LayoutNode> = (0..u16::from(self.max))
            .map(|i| self.star(tokens, shown.saturating_sub(i * 10).min(10)))
            .collect();
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(2),
                align: Align::Center,
                justify: Justify::Start,
                ..Stack::default()
            },
            VisualStyle::default(),
            stars,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::{
        LineLayout, MeasureText, PreparedTextHandle, ShapeKind, TextContent, TextStyle,
    };
    use nexus_theme_tokens::{BaseTokens, DarkTokens};

    struct NoText;

    impl MeasureText for NoText {
        fn prepare(&self, _: &TextContent, _: &TextStyle) -> PreparedTextHandle {
            PreparedTextHandle(0)
        }
        fn measure_width(&self, _: &PreparedTextHandle) -> FxPx {
            FxPx::ZERO
        }
        fn layout_lines(&self, _: &PreparedTextHandle, _: FxPx, _: Option<u32>) -> LineLayout {
            LineLayout { lines: Vec::new(), natural_width: FxPx::ZERO }
        }
    }

    fn kids(node: &LayoutNode) -> &[LayoutNode] {
        match node {
            LayoutNode::Stack(_, _, c) => c,
            _ => panic!("a Stack"),
        }
    }

    fn visual(node: &LayoutNode) -> &VisualStyle {
        match node {
            LayoutNode::Stack(_, v, _) => v,
            _ => panic!("a Stack"),
        }
    }

    #[test]
    fn renders_max_stars_filled_up_to_value() {
        let t = BaseTokens;
        let node = Rating::new().value(3).max(5).id("stars").build(&t);
        match &node {
            LayoutNode::Stack(stack, _, stars) => {
                assert_eq!(stack.id, Some("stars"));
                assert_eq!(stack.gap, FxPx::new(2));
                assert_eq!(stars.len(), 5);
                assert_eq!(kids(&stars[0]).len(), 3, "body + outline + filled overlay");
                assert_eq!(kids(&stars[3]).len(), 2, "empty: body + outline");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn an_empty_star_is_a_fill_body_with_a_separator_outline() {
        let t = DarkTokens;
        let node = Rating::new().value(0).build(&t);
        let star = &kids(&node)[0];
        let body = visual(&kids(star)[0]);
        assert_eq!(body.background, Some(t.color(ColorToken::Fill)));
        assert!(matches!(&body.shape, ShapeKind::Path(p) if p.points.len() == 10));
        let outline = visual(&kids(star)[1]);
        assert_eq!(outline.background, Some(t.color(ColorToken::Separator)));
        assert!(matches!(&outline.shape, ShapeKind::Stroke { width_milli: 50, .. }));
    }

    #[test]
    fn filled_stars_take_warning_or_the_colour_given() {
        let t = BaseTokens;
        let node = Rating::new().value(1).build(&t);
        let filled = &kids(&kids(&node)[0])[2];
        assert_eq!(visual(&kids(filled)[0]).background, Some(t.color(ColorToken::Warning)));
        let red = Rating::new().value(1).color(ColorToken::Danger).build(&t);
        let filled = &kids(&kids(&red)[0])[2];
        assert_eq!(visual(&kids(filled)[0]).background, Some(t.color(ColorToken::Danger)));
    }

    #[test]
    fn a_fraction_clips_the_filled_star() {
        let t = BaseTokens;
        let row = Rating::new().value_tenths(25).size(20).build(&t);
        let boxes =
            LayoutEngine::new().layout(&row, FxPx::new(400), &NoText).expect("layout").boxes;
        let clips: Vec<_> = boxes
            .iter()
            .filter(|b| b.rect.height.0 == 20 && b.rect.width.0 < 20)
            .map(|b| b.rect.width.0)
            .collect();
        assert_eq!(clips, alloc::vec![10], "the third star is half filled");
        assert_eq!(kids(&kids(&row)[1])[2].item().position, Position::Absolute);
    }

    #[test]
    fn defaults_and_clamps() {
        let t = BaseTokens;
        let node = Rating::new().value(9).max(4).build(&t);
        assert_eq!(kids(&node).len(), 4);
        match &kids(&node)[0] {
            LayoutNode::Stack(stack, _, _) => assert_eq!(stack.min_width, Some(FxPx::new(22))),
            _ => panic!(),
        }
    }
}
