// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Icon` — the design-system symbol primitive, modelled on SwiftUI's SF Symbols:
//! a **scalable vector symbol** that is sized from the type scale (or an explicit
//! px) and tinted by a semantic color. A pure builder producing a
//! `LayoutNode::Stack` whose `VisualStyle.shape` is a normalized `Path` (a
//! `0..1000` viewbox the renderer scales to the node's box — resolution
//! independent). DSL-emittable.
//!
//! The built-in [`Symbol`] set covers the essential UI glyphs (single filled
//! outlines). The full curated symbol library (Lucide SVG → normalized `Path`
//! at build time) fills the SAME `Icon` API later — no API churn — and the
//! renderer already draws `ShapeKind::Path`, so it needs no core change.

extern crate alloc;

use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    PathPoint, PathShape, ShapeKind, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, Tokens, TypographyToken};

/// A built-in symbol (normalized `0..1000` filled outline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    Plus,
    Minus,
    Close,
    ChevronRight,
    ChevronDown,
    ChevronLeft,
    ChevronUp,
    Star,
}

/// `(x_milli, y_milli)` outline points (viewbox `0..1000`, y down).
type Pts = &'static [(u16, u16)];

const PLUS: Pts = &[
    (400, 150), (600, 150), (600, 400), (850, 400), (850, 600), (600, 600),
    (600, 850), (400, 850), (400, 600), (150, 600), (150, 400), (400, 400),
];
const MINUS: Pts = &[(150, 400), (850, 400), (850, 600), (150, 600)];
const CLOSE: Pts = &[
    (180, 290), (290, 180), (500, 390), (710, 180), (820, 290), (610, 500),
    (820, 710), (710, 820), (500, 610), (290, 820), (180, 710), (390, 500),
];
const CHEVRON_RIGHT: Pts = &[(320, 180), (760, 500), (320, 820), (250, 730), (560, 500), (250, 270)];
const CHEVRON_LEFT: Pts = &[(680, 180), (240, 500), (680, 820), (750, 730), (440, 500), (750, 270)];
const CHEVRON_DOWN: Pts = &[(180, 320), (500, 760), (820, 320), (730, 250), (500, 560), (270, 250)];
const CHEVRON_UP: Pts = &[(180, 680), (500, 240), (820, 680), (730, 750), (500, 440), (270, 750)];
const STAR: Pts = &[
    (500, 100), (594, 371), (880, 376), (652, 549), (735, 824), (500, 660),
    (265, 824), (348, 549), (120, 376), (406, 371),
];

impl Symbol {
    fn points(self) -> Pts {
        match self {
            Symbol::Plus => PLUS,
            Symbol::Minus => MINUS,
            Symbol::Close => CLOSE,
            Symbol::ChevronRight => CHEVRON_RIGHT,
            Symbol::ChevronLeft => CHEVRON_LEFT,
            Symbol::ChevronDown => CHEVRON_DOWN,
            Symbol::ChevronUp => CHEVRON_UP,
            Symbol::Star => STAR,
        }
    }

    /// The symbol's normalized filled outline.
    pub fn path(self) -> PathShape {
        let mut points = alloc::vec::Vec::with_capacity(self.points().len());
        for &(x, y) in self.points() {
            points.push(PathPoint::new(x, y));
        }
        PathShape { points, closed: true }
    }
}

/// A symbol, sized and tinted.
#[derive(Debug, Clone)]
pub struct Icon {
    symbol: Symbol,
    size: FxPx,
    color: ColorToken,
    id: Option<&'static str>,
}

impl Icon {
    /// A `16px`, `OnSurface` symbol.
    pub fn new(symbol: Symbol) -> Self {
        Self { symbol, size: FxPx::new(16), color: ColorToken::OnSurface, id: None }
    }

    /// Explicit pixel size.
    pub fn size(mut self, px: i32) -> Self {
        self.size = FxPx::new(px.max(1));
        self
    }
    /// Size from the type scale (matches adjacent text — the SwiftUI behaviour).
    pub fn type_size(mut self, tokens: &dyn Tokens, token: TypographyToken) -> Self {
        self.size = tokens.type_size(token);
        self
    }
    /// Tint (the "foreground color").
    pub fn color(mut self, color: ColorToken) -> Self {
        self.color = color;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Build the icon node (a sized, tinted vector symbol).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let visual = VisualStyle {
            background: Some(tokens.color(self.color)),
            shape: ShapeKind::Path(self.symbol.path()),
            corner_radius: CornerRadius::uniform(FxPx::ZERO),
            ..VisualStyle::default()
        };
        let d = Some(self.size);
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem::default(),
            },
            visual,
            alloc::vec![],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn every_symbol_has_a_closed_outline_in_the_viewbox() {
        for sym in [
            Symbol::Plus,
            Symbol::Minus,
            Symbol::Close,
            Symbol::ChevronRight,
            Symbol::ChevronLeft,
            Symbol::ChevronDown,
            Symbol::ChevronUp,
            Symbol::Star,
        ] {
            let path = sym.path();
            assert!(path.closed);
            assert!(path.points.len() >= 3, "{sym:?} needs a polygon");
            for p in &path.points {
                assert!(p.x_milli <= 1000 && p.y_milli <= 1000, "{sym:?} out of viewbox");
            }
        }
    }

    #[test]
    fn builds_a_sized_tinted_path_node() {
        let t = BaseTokens;
        match Icon::new(Symbol::Star).size(24).color(ColorToken::Warning).id("rate").build(&t) {
            LayoutNode::Stack(stack, v, _) => {
                assert_eq!(stack.id, Some("rate"));
                assert_eq!(stack.min_width, Some(FxPx::new(24)));
                assert_eq!(v.background, Some(t.color(ColorToken::Warning)));
                assert!(matches!(v.shape, ShapeKind::Path(_)));
            }
            _ => panic!("Icon must build a Stack"),
        }
    }

    #[test]
    fn type_size_matches_the_scale() {
        let t = BaseTokens;
        let icon = Icon::new(Symbol::Plus).type_size(&t, TypographyToken::Base);
        assert_eq!(icon.size, t.type_size(TypographyToken::Base));
    }
}
