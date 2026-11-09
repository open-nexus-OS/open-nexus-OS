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
//! outlines); the theme-linked line set ([`LucideSymbol`], imported at build time) is the
//! design artifacts' icon renderer: stroked polylines at the set's width, or at a per-icon
//! width ([`Icon::stroke_tenths`] — the chrome's 2.2–3). [`with_ink`] is `currentColor`: a
//! container re-inks the glyph or text it holds (a selected row turns its icon `onAccent`).

extern crate alloc;

use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    PathPoint, PathShape, Rgba8, ShapeKind, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, TextStyleToken, Tokens};

// Imported Lucide line symbols: `LucideSymbol` + `lucide_contours`.
include!(concat!(env!("OUT_DIR"), "/lucide.rs"));

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
    (400, 150),
    (600, 150),
    (600, 400),
    (850, 400),
    (850, 600),
    (600, 600),
    (600, 850),
    (400, 850),
    (400, 600),
    (150, 600),
    (150, 400),
    (400, 400),
];
const MINUS: Pts = &[(150, 400), (850, 400), (850, 600), (150, 600)];
const CLOSE: Pts = &[
    (180, 290),
    (290, 180),
    (500, 390),
    (710, 180),
    (820, 290),
    (610, 500),
    (820, 710),
    (710, 820),
    (500, 610),
    (290, 820),
    (180, 710),
    (390, 500),
];
const CHEVRON_RIGHT: Pts =
    &[(320, 180), (760, 500), (320, 820), (250, 730), (560, 500), (250, 270)];
const CHEVRON_LEFT: Pts = &[(680, 180), (240, 500), (680, 820), (750, 730), (440, 500), (750, 270)];
const CHEVRON_DOWN: Pts = &[(180, 320), (500, 760), (820, 320), (730, 250), (500, 560), (270, 250)];
const CHEVRON_UP: Pts = &[(180, 680), (500, 240), (820, 680), (730, 750), (500, 440), (270, 750)];
const STAR: Pts = &[
    (500, 100),
    (594, 371),
    (880, 376),
    (652, 549),
    (735, 824),
    (500, 660),
    (265, 824),
    (348, 549),
    (120, 376),
    (406, 371),
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
    shape: ShapeKind,
    size: FxPx,
    color: ColorToken,
    id: Option<&'static str>,
}

impl Icon {
    /// A `16px`, `Label` built-in symbol.
    pub fn new(symbol: Symbol) -> Self {
        Self {
            shape: ShapeKind::Path(symbol.path()),
            size: FxPx::new(16),
            color: ColorToken::Label,
            id: None,
        }
    }

    /// An imported line symbol: its polylines, stroked at the set's own width with round caps
    /// and joins, anti-aliased (`ShapeKind::Stroke`).
    pub fn lucide(sym: LucideSymbol) -> Self {
        let paths = lucide_contours(sym)
            .iter()
            .map(|c| PathShape {
                points: c.iter().map(|&(x, y)| PathPoint::new(x, y)).collect(),
                closed: false,
            })
            .collect();
        Self {
            shape: ShapeKind::Stroke { paths, width_milli: LUCIDE_STROKE_MILLI },
            size: FxPx::new(16),
            color: ColorToken::Label,
            id: None,
        }
    }

    /// A custom line symbol — the design artifacts' raw-path icons (a list row's check, a
    /// reorder handle, the window menu's tile glyphs): polylines in the normalized `0..1000`
    /// viewbox (a 24-unit path coordinate `c` is `c * 1000 / 24`), stroked like the imported
    /// set — its width, round caps and joins.
    pub fn polylines(contours: &[&[(u16, u16)]]) -> Self {
        let paths = contours
            .iter()
            .map(|c| PathShape {
                points: c.iter().map(|&(x, y)| PathPoint::new(x, y)).collect(),
                closed: false,
            })
            .collect();
        Self {
            shape: ShapeKind::Stroke { paths, width_milli: LUCIDE_STROKE_MILLI },
            size: FxPx::new(16),
            color: ColorToken::Label,
            id: None,
        }
    }

    /// The stroke width of a line symbol in tenths of the set's 24-unit viewbox (`22` = the
    /// design's `strokeWidth 2.2`); the set's own width is `20`. A filled symbol has no stroke
    /// and ignores it.
    pub fn stroke_tenths(mut self, tenths: u16) -> Self {
        if let ShapeKind::Stroke { width_milli, .. } = &mut self.shape {
            *width_milli = stroke_milli(tenths);
        }
        self
    }
    /// Explicit pixel size.
    pub fn size(mut self, px: i32) -> Self {
        self.size = FxPx::new(px.max(1));
        self
    }
    /// Size from a text style (matches adjacent text — the SwiftUI behaviour).
    pub fn type_size(mut self, token: TextStyleToken) -> Self {
        self.size = FxPx::new(i32::from(token.spec().size));
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
            shape: self.shape,
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
                text_fit: None,
            },
            visual,
            alloc::vec![],
        )
    }
}

/// A stroke width in tenths of the 24-unit viewbox → the normalized `0..1000` viewbox.
const fn stroke_milli(tenths: u16) -> u16 {
    ((tenths as u32 * 1000 + 120) / 240) as u16
}

/// `currentColor`: re-ink a built glyph (an [`Icon`]'s vector shape) or a text run in `color`.
/// A container that colours its content uses this — a destructive row's leading icon follows the
/// title into `danger`, a selected item turns its icon `onAccent`, a dimmed rail icon takes the
/// label at 62 %. Only the node itself is touched; any other node (an avatar, a control, a box)
/// keeps its own colours.
#[must_use]
pub fn with_ink(node: LayoutNode, color: Rgba8) -> LayoutNode {
    match node {
        LayoutNode::Stack(stack, mut visual, children) if is_glyph(&visual.shape) => {
            visual.background = Some(color);
            LayoutNode::Stack(stack, visual, children)
        }
        LayoutNode::Text(mut text, visual) => {
            text.style.color = color;
            LayoutNode::Text(text, visual)
        }
        other => other,
    }
}

/// Whether a shape is a glyph drawn in its node's ink (not a filled box).
fn is_glyph(shape: &ShapeKind) -> bool {
    matches!(
        shape,
        ShapeKind::Path(_)
            | ShapeKind::Vector(_)
            | ShapeKind::Stroke { .. }
            | ShapeKind::TriangleUp
            | ShapeKind::TriangleDown
    )
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
    fn imported_lucide_symbols_are_stroked_polylines() {
        let t = BaseTokens;
        // "menu" (hamburger) = 3 lines → 3 two-point polylines, at the set's stroke width
        // (2 of 24 → 83 of 1000).
        match Icon::lucide(LucideSymbol::Menu).size(24).build(&t) {
            LayoutNode::Stack(_, v, _) => match v.shape {
                ShapeKind::Stroke { paths, width_milli } => {
                    assert_eq!(paths.len(), 3, "3 hamburger lines");
                    assert!(paths.iter().all(|p| p.points.len() == 2));
                    assert_eq!(width_milli, 83);
                }
                _ => panic!("lucide icon must be a Stroke"),
            },
            _ => panic!(),
        }
        // "check" = one polyline of three points (two segments joined, not two loose quads).
        match Icon::lucide(LucideSymbol::Check).build(&t) {
            LayoutNode::Stack(_, v, _) => match v.shape {
                ShapeKind::Stroke { paths, .. } => {
                    assert_eq!(paths.len(), 1);
                    assert_eq!(paths[0].points.len(), 3);
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn a_line_symbol_takes_a_per_icon_stroke() {
        let t = BaseTokens;
        // The set's own 2 → 83; the chrome's 2.5 → 104; a filled symbol ignores it.
        assert_eq!(stroke_milli(20), LUCIDE_STROKE_MILLI);
        match Icon::lucide(LucideSymbol::Minus).stroke_tenths(25).build(&t) {
            LayoutNode::Stack(_, v, _) => {
                assert!(matches!(v.shape, ShapeKind::Stroke { width_milli: 104, .. }));
            }
            _ => panic!(),
        }
        match Icon::new(Symbol::Plus).stroke_tenths(25).build(&t) {
            LayoutNode::Stack(_, v, _) => assert!(matches!(v.shape, ShapeKind::Path(_))),
            _ => panic!(),
        }
    }

    #[test]
    fn a_custom_path_is_a_stroked_symbol() {
        // The design's reorder handle: three 16-unit lines at y 8/12/16 of the 24 viewbox.
        const HANDLE: &[&[(u16, u16)]] =
            &[&[(167, 333), (833, 333)], &[(167, 500), (833, 500)], &[(167, 667), (833, 667)]];
        match Icon::polylines(HANDLE).size(18).build(&BaseTokens) {
            LayoutNode::Stack(stack, v, _) => {
                assert_eq!(stack.min_width, Some(FxPx::new(18)));
                match v.shape {
                    ShapeKind::Stroke { paths, width_milli } => {
                        assert_eq!(paths.len(), 3);
                        assert_eq!(width_milli, LUCIDE_STROKE_MILLI);
                    }
                    _ => panic!("a custom path is stroked"),
                }
            }
            _ => panic!(),
        }
    }

    #[test]
    fn a_rounded_rect_keeps_its_corners() {
        // `square` is `<rect … rx="2"/>`: a closed outline whose corners are arcs, so it has
        // more points than the four corners of a sharp box (plus the closing point).
        let contours = lucide_contours(LucideSymbol::Square);
        assert_eq!(contours.len(), 1);
        assert!(contours[0].len() > 5, "rx must round the corners");
        assert_eq!(contours[0].first(), contours[0].last(), "the outline closes");
    }

    #[test]
    fn ink_recolours_glyphs_and_text_only() {
        let t = BaseTokens;
        let red = t.color(ColorToken::Danger);
        match with_ink(Icon::lucide(LucideSymbol::Check).build(&t), red) {
            LayoutNode::Stack(_, v, _) => assert_eq!(v.background, Some(red)),
            _ => panic!(),
        }
        let boxed = LayoutNode::Stack(
            Stack::default(),
            VisualStyle { background: Some(t.color(ColorToken::Fill)), ..VisualStyle::default() },
            alloc::vec![],
        );
        match with_ink(boxed, red) {
            LayoutNode::Stack(_, v, _) => {
                assert_eq!(v.background, Some(t.color(ColorToken::Fill)), "a box keeps its fill")
            }
            _ => panic!(),
        }
    }

    #[test]
    fn type_size_matches_the_scale() {
        let icon = Icon::new(Symbol::Plus).type_size(TextStyleToken::Body);
        assert_eq!(icon.size, FxPx::new(i32::from(TextStyleToken::Body.spec().size)));
    }
}
