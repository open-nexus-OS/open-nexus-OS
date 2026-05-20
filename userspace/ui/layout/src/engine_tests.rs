#[cfg(test)]
mod tests {
    use crate::engine::LayoutEngine;
    use crate::error::LayoutError;
    use nexus_layout_types::{
        Align, Direction, EdgeInsets, FlexItem, FontWeight, Fraction, FxPx, Grid, Justify,
        LayoutNode, LineHeight, LineLayout, LineMetrics, MeasureText, PreparedTextHandle,
        TextAlign, TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
    };

    struct MockMeasure {
        char_width: FxPx,
    }
    impl MeasureText for MockMeasure {
        fn prepare(&self, content: &TextContent, _: &TextStyle) -> PreparedTextHandle {
            PreparedTextHandle(content.as_str().chars().count())
        }
        fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx {
            self.char_width * handle.0 as i32
        }
        fn layout_lines(
            &self,
            handle: &PreparedTextHandle,
            width: FxPx,
            _: Option<u32>,
        ) -> LineLayout {
            let natural_width = self.measure_width(handle);
            let line_width = natural_width.min(width);
            LineLayout {
                lines: vec![LineMetrics {
                    text_range: 0..handle.0,
                    width: line_width,
                    baseline: FxPx::new(16),
                    height: FxPx::new(20),
                }],
                natural_width,
            }
        }
    }
    fn px(v: i32) -> FxPx {
        FxPx::new(v)
    }
    fn text_style() -> TextStyle {
        TextStyle {
            font_size: px(16),
            font_weight: FontWeight::Regular,
            line_height: LineHeight::Absolute(px(20)),
            text_align: TextAlign::Left,
            color: nexus_layout_types::Rgba8::WHITE,
            white_space: WhiteSpace::Normal,
        }
    }
    fn txt(s: &str) -> LayoutNode {
        LayoutNode::Text(
            TextNode {
                id: None,
                content: TextContent::new(s),
                style: text_style(),
                item: FlexItem::default(),
                max_lines: None,
                min_width: None,
                max_width: None,
            },
            VisualStyle::default(),
        )
    }
    fn s_col(c: Vec<LayoutNode>) -> LayoutNode {
        LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: None,
                direction: Direction::Column,
                gap: px(4),
                padding: EdgeInsets::all(px(8)),
                align: Align::Start,
                justify: Justify::Start,
                overflow: nexus_layout_types::Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            c,
        )
    }

    #[test]
    fn empty() {
        let r = LayoutEngine::new()
            .layout(&s_col(vec![]), px(800), &MockMeasure { char_width: px(0) })
            .unwrap();
        assert_eq!(r.content_height, px(16));
    }
    #[test]
    fn text() {
        let r = LayoutEngine::new()
            .layout(&txt("x"), px(800), &MockMeasure { char_width: px(100) })
            .unwrap();
        assert_eq!(r.boxes[0].rect.width, px(100));
    }
    #[test]
    fn col() {
        let r = LayoutEngine::new()
            .layout(&s_col(vec![txt("a"), txt("b")]), px(200), &MockMeasure { char_width: px(50) })
            .unwrap();
        assert_eq!(r.boxes.len(), 3);
        assert_eq!(r.boxes[1].rect.y, px(8));
        assert_eq!(r.boxes[2].rect.y, px(32));
    }
    #[test]
    fn row() {
        let s = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: None,
                direction: Direction::Row,
                gap: px(4),
                padding: EdgeInsets::all(px(8)),
                align: Align::Start,
                justify: Justify::Start,
                overflow: nexus_layout_types::Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            vec![txt("a"), txt("b")],
        );
        let r =
            LayoutEngine::new().layout(&s, px(200), &MockMeasure { char_width: px(30) }).unwrap();
        assert_eq!(r.boxes[1].rect.x, px(8));
        assert_eq!(r.boxes[2].rect.x, px(42));
    }
    #[test]
    fn max_nodes() {
        let e = LayoutEngine::with_limits(3, 64);
        let r = e.layout(
            &s_col(vec![txt("a"), txt("b"), txt("c"), txt("d")]),
            px(100),
            &MockMeasure { char_width: px(10) },
        );
        assert!(matches!(r, Err(LayoutError::TooManyNodes { .. })));
    }
    #[test]
    fn grid() {
        let g = LayoutNode::Grid(
            Grid {
                id: None,
                columns: vec![Fraction(1), Fraction(2), Fraction(1)],
                gap: px(8),
                row_gap: Some(px(4)),
                padding: EdgeInsets::all(px(8)),
                overflow: nexus_layout_types::Overflow::Visible,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            vec![txt("a"), txt("b"), txt("c"), txt("d"), txt("e")],
        );
        let r =
            LayoutEngine::new().layout(&g, px(400), &MockMeasure { char_width: px(80) }).unwrap();
        assert_eq!(r.boxes.len(), 6);
        assert_eq!(r.boxes[1].rect.y, px(8));
        assert_eq!(r.boxes[4].rect.y, px(32));
    }
    #[test]
    fn grid_div0() {
        let g = LayoutNode::Grid(
            Grid {
                id: None,
                columns: vec![Fraction(0), Fraction(0)],
                gap: px(8),
                row_gap: None,
                padding: EdgeInsets::zero(),
                overflow: nexus_layout_types::Overflow::Visible,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            vec![],
        );
        assert!(matches!(
            LayoutEngine::new().layout(&g, px(400), &MockMeasure { char_width: px(80) }),
            Err(LayoutError::DivByZero)
        ));
    }
    #[test]
    fn visual_style_propagated() {
        let vs = VisualStyle {
            background: Some(nexus_layout_types::Rgba8::WHITE),
            ..Default::default()
        };
        let node = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: None,
                direction: Direction::Column,
                gap: px(0),
                padding: EdgeInsets::zero(),
                align: Align::Start,
                justify: Justify::Start,
                overflow: nexus_layout_types::Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            vs.clone(),
            vec![],
        );
        let r =
            LayoutEngine::new().layout(&node, px(100), &MockMeasure { char_width: px(0) }).unwrap();
        assert_eq!(r.boxes[0].visual.background, Some(nexus_layout_types::Rgba8::WHITE));
    }

    #[test]
    fn column_shrink_respects_zero_shrink_children() {
        let fixed = LayoutNode::Text(
            TextNode {
                id: Some("fixed"),
                content: TextContent::new("fixed"),
                style: text_style(),
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                max_lines: None,
                min_width: None,
                max_width: None,
            },
            VisualStyle::default(),
        );
        let flex = LayoutNode::Text(
            TextNode {
                id: Some("flex"),
                content: TextContent::new("flex"),
                style: text_style(),
                item: FlexItem { flex_shrink: 1, ..FlexItem::default() },
                max_lines: None,
                min_width: None,
                max_width: None,
            },
            VisualStyle::default(),
        );
        let root = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("root"),
                direction: Direction::Column,
                gap: px(4),
                padding: EdgeInsets::all(px(8)),
                align: Align::Stretch,
                justify: Justify::Start,
                overflow: nexus_layout_types::Overflow::Visible,
                flex_wrap: false,
                min_width: Some(px(120)),
                max_width: Some(px(120)),
                min_height: Some(px(40)),
                max_height: Some(px(40)),
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            vec![fixed, flex],
        );
        let r = LayoutEngine::new()
            .layout(&root, px(120), &MockMeasure { char_width: px(10) })
            .unwrap();
        let fixed = r.boxes.iter().find(|b| b.id == Some("fixed")).unwrap();
        let flex = r.boxes.iter().find(|b| b.id == Some("flex")).unwrap();
        assert_eq!(fixed.rect.height, px(20));
        assert!(flex.rect.height < px(20));
        assert_eq!(flex.rect.y, fixed.rect.y + fixed.rect.height + px(4));
    }
}
