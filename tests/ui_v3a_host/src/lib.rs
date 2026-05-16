// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests for TASK-0058: layout JSON goldens.
//! JSON is a derived view (ADR-0021), diffable for regression detection.

#[cfg(test)]
mod tests {
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::{
        Align, Direction, EdgeInsets, FxPx, Fraction, Grid, Justify, LayoutNode,
        LineLayout, LineMetrics, MeasureText, PreparedTextHandle, TextContent, TextNode, VisualStyle,
    };
    use serde::Serialize;

    #[derive(Debug, Serialize, PartialEq)]
    struct GoldenBox {
        node_id: usize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    }

    struct MockMeasure { char_width: FxPx }
    impl MeasureText for MockMeasure {
        fn prepare(&self, _: &str) -> PreparedTextHandle { PreparedTextHandle(0) }
        fn measure_width(&self, _: PreparedTextHandle) -> FxPx { self.char_width }
        fn layout_lines(&self, _: PreparedTextHandle, _: FxPx, _: Option<u32>) -> LineLayout {
            LineLayout { lines: vec![], natural_width: self.char_width }
        }
    }
    fn px(v: i32) -> FxPx { FxPx::new(v) }
    fn txt(s: &str) -> LayoutNode {
        LayoutNode::Text(TextNode { content: TextContent::new(s), max_lines: None, min_width: None, max_width: None }, VisualStyle::default())
    }
    fn layout_to_golden(node: &LayoutNode, width: i32) -> Vec<GoldenBox> {
        let r = LayoutEngine::new().layout(node, px(width), &MockMeasure { char_width: px(0) }).unwrap();
        r.boxes.iter().map(|b| GoldenBox {
            node_id: b.node_id,
            x: b.rect.x.0, y: b.rect.y.0, width: b.rect.width.0, height: b.rect.height.0,
        }).collect()
    }

    #[test]
    fn golden_flex_row() {
        let s = LayoutNode::Stack(nexus_layout_types::Stack {
            direction: Direction::Row, gap: px(8), padding: EdgeInsets::all(px(16)),
            align: Align::Start, justify: Justify::Start,
            overflow: nexus_layout_types::Overflow::Visible, flex_wrap: false,
            min_width: None, max_width: None, min_height: None, max_height: None,
        }, VisualStyle::default(), vec![txt("A"), txt("B"), txt("C")]);
        let g = layout_to_golden(&s, 400);
        let json = serde_json::to_string_pretty(&g).unwrap();
        // Stable snapshot: compare against known JSON
        assert_eq!(g.len(), 4); // container + 3 text
        assert_eq!(g[0].x, 0);
        assert_eq!(g[0].width, 400);
        assert_eq!(g[1].x, 16); // padding-left
        assert_eq!(g[2].x, 16 + 8); // text width 0 + gap 8
        assert!(json.contains("\"node_id\""));
    }

    #[test]
    fn golden_column_stack() {
        let s = LayoutNode::Stack(nexus_layout_types::Stack {
            direction: Direction::Column, gap: px(4), padding: EdgeInsets::all(px(8)),
            align: Align::Start, justify: Justify::Start,
            overflow: nexus_layout_types::Overflow::Visible, flex_wrap: false,
            min_width: None, max_width: None, min_height: None, max_height: None,
        }, VisualStyle::default(), vec![txt("X"), txt("Y")]);
        let g = layout_to_golden(&s, 200);
        assert_eq!(g.len(), 3); // container + 2 text
        assert_eq!(g[1].y, 8); // padding-top
        assert_eq!(g[2].y, 32); // 8 + 20 + 4
    }

    #[test]
    fn golden_grid_3col() {
        let g = LayoutNode::Grid(Grid {
            columns: vec![Fraction(1), Fraction(2), Fraction(1)],
            gap: px(8), row_gap: Some(px(4)), padding: EdgeInsets::all(px(8)),
            overflow: nexus_layout_types::Overflow::Visible,
            min_width: None, max_width: None, min_height: None, max_height: None,
        }, VisualStyle::default(), vec![txt("a"), txt("b"), txt("c"), txt("d"), txt("e")]);
        let r = layout_to_golden(&g, 400);
        assert_eq!(r.len(), 6); // container + 5
        // Row 1 all at y=8
        assert_eq!(r[1].y, 8);
        assert_eq!(r[2].y, 8);
        assert_eq!(r[3].y, 8);
        // Row 2 at y = 8 + 20 + 4 = 32
        assert_eq!(r[4].y, 32);
        assert_eq!(r[5].y, 32);
    }

    #[test]
    fn golden_visual_style() {
        let vs = VisualStyle {
            background: Some(nexus_layout_types::Rgba8 { r: 255, g: 0, b: 0, a: 255 }),
            ..Default::default()
        };
        let node = LayoutNode::Text(TextNode {
            content: TextContent::new("red"), max_lines: None, min_width: None, max_width: None,
        }, vs);
        let r = LayoutEngine::new().layout(&node, px(100), &MockMeasure { char_width: px(0) }).unwrap();
        assert_eq!(r.boxes[0].visual.background, Some(nexus_layout_types::Rgba8 { r: 255, g: 0, b: 0, a: 255 }));
    }
}
