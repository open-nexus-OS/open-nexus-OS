// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Line and filled glyphs drawn in the design artifacts' 24-unit icon grid — the ✓, ✕, dash,
//! chevron and star the control primitives draw themselves (their SVG paths, transcribed point
//! for point). A point is given in TENTHS of a grid unit (`(45, 125)` = 4.5, 12.5) so the
//! artifacts' one-decimal coordinates stay exact integers; the node scales the path to its box
//! (the renderer's normalized `0..1000` viewbox), so one glyph serves every size.

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, FlexItem, FxPx, Justify, LayoutNode, PathPoint, PathShape, Rgba8, ShapeKind,
    Stack, VisualStyle,
};

/// A point of the 24-unit grid in tenths of a unit.
pub type GridPoint = (u16, u16);

/// Tenths of the 24-unit grid → the renderer's `0..1000` viewbox (rounded).
#[must_use]
pub const fn grid_milli(tenths: u16) -> u16 {
    ((tenths as u32 * 1000 + 120) / 240) as u16
}

fn path(points: &[GridPoint], closed: bool) -> PathShape {
    let points: Vec<PathPoint> =
        points.iter().map(|&(x, y)| PathPoint::new(grid_milli(x), grid_milli(y))).collect();
    PathShape { points, closed }
}

/// A `size`-square box painting `shape` in `color` (a fully transparent colour paints nothing
/// but keeps the box — a hidden checkmark still holds its place). It never shrinks below its size.
fn glyph_box(shape: ShapeKind, size: FxPx, color: Rgba8) -> LayoutNode {
    let side = Some(size);
    LayoutNode::Stack(
        Stack {
            direction: Direction::Row,
            align: Align::Center,
            justify: Justify::Center,
            min_width: side,
            max_width: side,
            min_height: side,
            max_height: side,
            item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
            ..Stack::default()
        },
        VisualStyle { background: (color.a > 0).then_some(color), shape, ..VisualStyle::default() },
        Vec::new(),
    )
}

/// A stroked line glyph: each polyline in grid tenths (closed when `closed`), stroked
/// `stroke_tenths` wide with round caps and joins, `size` px square in `color`.
#[must_use]
pub fn stroke(
    paths: &[&[GridPoint]],
    closed: bool,
    stroke_tenths: u16,
    size: FxPx,
    color: Rgba8,
) -> LayoutNode {
    let paths = paths.iter().map(|p| path(p, closed)).collect();
    glyph_box(ShapeKind::Stroke { paths, width_milli: grid_milli(stroke_tenths) }, size, color)
}

/// A filled glyph: one closed contour in grid tenths, `size` px square in `color`.
#[must_use]
pub fn fill(points: &[GridPoint], size: FxPx, color: Rgba8) -> LayoutNode {
    glyph_box(ShapeKind::Path(path(points, true)), size, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_tenths_map_onto_the_viewbox() {
        assert_eq!(grid_milli(0), 0);
        assert_eq!(grid_milli(240), 1000);
        assert_eq!(grid_milli(120), 500);
        assert_eq!(grid_milli(26), 108, "a 2.6 stroke");
    }

    #[test]
    fn a_stroke_glyph_is_a_pinned_square_in_its_colour() {
        let ink = Rgba8::new(1, 2, 3, 255);
        match stroke(
            &[&[(60, 60), (180, 180)], &[(180, 60), (60, 180)]],
            false,
            26,
            FxPx::new(14),
            ink,
        ) {
            LayoutNode::Stack(stack, visual, children) => {
                assert_eq!(stack.min_width, Some(FxPx::new(14)));
                assert_eq!(stack.max_height, Some(FxPx::new(14)));
                assert_eq!(stack.item.flex_shrink, 0);
                assert_eq!(visual.background, Some(ink));
                assert!(children.is_empty());
                match visual.shape {
                    ShapeKind::Stroke { paths, width_milli } => {
                        assert_eq!(paths.len(), 2);
                        assert_eq!(width_milli, 108);
                        assert_eq!(paths[0].points[0], PathPoint::new(250, 250));
                    }
                    other => panic!("expected a stroke, got {other:?}"),
                }
            }
            _ => panic!("a glyph is a Stack"),
        }
    }

    #[test]
    fn a_transparent_glyph_keeps_its_box_but_paints_nothing() {
        match fill(&[(0, 0), (240, 0), (120, 240)], FxPx::new(8), Rgba8::new(0, 0, 0, 0)) {
            LayoutNode::Stack(stack, visual, _) => {
                assert_eq!(stack.min_width, Some(FxPx::new(8)));
                assert_eq!(visual.background, None);
            }
            _ => panic!(),
        }
    }
}
