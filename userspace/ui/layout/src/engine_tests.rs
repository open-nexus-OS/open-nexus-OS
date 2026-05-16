#[cfg(test)]
mod tests {
    use crate::engine::LayoutEngine;
    use crate::error::LayoutError;
    use nexus_layout_types::{
        Align, Direction, EdgeInsets, FxPx, Fraction, Grid, Justify, LayoutNode,
        LineLayout, MeasureText, PreparedTextHandle, TextContent, TextNode, VisualStyle,
    };

    struct MockMeasure { char_width: FxPx }
    impl MeasureText for MockMeasure {
        fn prepare(&self, _: &str) -> PreparedTextHandle { PreparedTextHandle(0) }
        fn measure_width(&self, _: PreparedTextHandle) -> FxPx { self.char_width }
        fn layout_lines(&self, _: PreparedTextHandle, _: FxPx, _: Option<u32>) -> LineLayout {
            LineLayout { lines: vec![], natural_width: self.char_width }
        }
    }
    fn px(v: i32) -> FxPx { FxPx::new(v) }
    fn txt(s: &str) -> LayoutNode { LayoutNode::Text(TextNode { content: TextContent::new(s), max_lines: None, min_width: None, max_width: None }, VisualStyle::default()) }
    fn s_col(c: Vec<LayoutNode>) -> LayoutNode {
        LayoutNode::Stack(nexus_layout_types::Stack {
            direction: Direction::Column, gap: px(4), padding: EdgeInsets::all(px(8)),
            align: Align::Start, justify: Justify::Start,
            overflow: nexus_layout_types::Overflow::Visible, flex_wrap: false,
            min_width: None, max_width: None, min_height: None, max_height: None,
        }, VisualStyle::default(), c)
    }

    #[test] fn empty() {
        let r = LayoutEngine::new().layout(&s_col(vec![]), px(800), &MockMeasure{char_width:px(0)}).unwrap();
        assert_eq!(r.content_height, px(16));
    }
    #[test] fn text() {
        let r = LayoutEngine::new().layout(&txt("x"), px(800), &MockMeasure{char_width:px(100)}).unwrap();
        assert_eq!(r.boxes[0].rect.width, px(100));
    }
    #[test] fn col() {
        let r = LayoutEngine::new().layout(&s_col(vec![txt("a"),txt("b")]), px(200), &MockMeasure{char_width:px(50)}).unwrap();
        assert_eq!(r.boxes.len(), 3);
        assert_eq!(r.boxes[1].rect.y, px(8));
        assert_eq!(r.boxes[2].rect.y, px(32));
    }
    #[test] fn row() {
        let s = LayoutNode::Stack(nexus_layout_types::Stack {
            direction: Direction::Row, gap: px(4), padding: EdgeInsets::all(px(8)),
            align: Align::Start, justify: Justify::Start,
            overflow: nexus_layout_types::Overflow::Visible, flex_wrap: false,
            min_width: None, max_width: None, min_height: None, max_height: None,
        }, VisualStyle::default(), vec![txt("a"),txt("b")]);
        let r = LayoutEngine::new().layout(&s, px(200), &MockMeasure{char_width:px(30)}).unwrap();
        assert_eq!(r.boxes[1].rect.x, px(8));
        assert_eq!(r.boxes[2].rect.x, px(42));
    }
    #[test] fn max_nodes() {
        let e = LayoutEngine::with_limits(3, 64);
        let r = e.layout(&s_col(vec![txt("a"),txt("b"),txt("c"),txt("d")]), px(100), &MockMeasure{char_width:px(10)});
        assert!(matches!(r, Err(LayoutError::TooManyNodes{..})));
    }
    #[test] fn grid() {
        let g = LayoutNode::Grid(Grid {
            columns: vec![Fraction(1),Fraction(2),Fraction(1)], gap: px(8), row_gap: Some(px(4)),
            padding: EdgeInsets::all(px(8)), overflow: nexus_layout_types::Overflow::Visible,
            min_width: None, max_width: None, min_height: None, max_height: None,
        }, VisualStyle::default(), vec![txt("a"),txt("b"),txt("c"),txt("d"),txt("e")]);
        let r = LayoutEngine::new().layout(&g, px(400), &MockMeasure{char_width:px(80)}).unwrap();
        assert_eq!(r.boxes.len(), 6);
        assert_eq!(r.boxes[1].rect.y, px(8));
        assert_eq!(r.boxes[4].rect.y, px(32));
    }
    #[test] fn grid_div0() {
        let g = LayoutNode::Grid(Grid {
            columns: vec![Fraction(0),Fraction(0)], gap: px(8), row_gap: None, padding: EdgeInsets::zero(),
            overflow: nexus_layout_types::Overflow::Visible,
            min_width: None, max_width: None, min_height: None, max_height: None,
        }, VisualStyle::default(), vec![]);
        assert!(matches!(LayoutEngine::new().layout(&g, px(400), &MockMeasure{char_width:px(80)}), Err(LayoutError::DivByZero)));
    }
    #[test] fn visual_style_propagated() {
        let vs = VisualStyle { background: Some(nexus_layout_types::Rgba8::WHITE), ..Default::default() };
        let node = LayoutNode::Stack(nexus_layout_types::Stack {
            direction: Direction::Column, gap: px(0), padding: EdgeInsets::zero(),
            align: Align::Start, justify: Justify::Start,
            overflow: nexus_layout_types::Overflow::Visible, flex_wrap: false,
            min_width: None, max_width: None, min_height: None, max_height: None,
        }, vs.clone(), vec![]);
        let r = LayoutEngine::new().layout(&node, px(100), &MockMeasure{char_width:px(0)}).unwrap();
        assert_eq!(r.boxes[0].visual.background, Some(nexus_layout_types::Rgba8::WHITE));
    }
}
