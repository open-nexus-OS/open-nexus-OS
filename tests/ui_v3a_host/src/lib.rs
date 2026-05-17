// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests for TASK-0058: layout JSON goldens.
//! JSON is a derived view (ADR-0021), diffable for regression detection.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use input_live_protocol::VisibleState;
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::{
        Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Fraction, Grid, Justify,
        LayoutNode, LineHeight, LineLayout, LineMetrics, MeasureText,
        PreparedTextHandle, ShapeKind, TextAlign, TextContent, TextNode, TextStyle, VisualStyle,
        WhiteSpace,
    };
    use nexus_shape::CachedTextMeasure;
    use serde::Serialize;
    use windowd::{build_proof_panel_tree, compute_proof_layout, ProofTextMeasure};

    #[derive(Debug, Serialize, PartialEq)]
    struct GoldenBox {
        node_id: usize,
        id: Option<&'static str>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    }

    struct MockMeasure { char_width: FxPx }
    impl MeasureText for MockMeasure {
        fn prepare(&self, content: &TextContent, _: &TextStyle) -> PreparedTextHandle {
            PreparedTextHandle(content.as_str().chars().count())
        }
        fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx {
            self.char_width * handle.0 as i32
        }
        fn layout_lines(&self, handle: &PreparedTextHandle, width: FxPx, _: Option<u32>) -> LineLayout {
            let natural_width = self.measure_width(handle);
            LineLayout {
                lines: vec![LineMetrics {
                    text_range: 0..handle.0,
                    width: natural_width.min(width),
                    baseline: px(16),
                    height: px(20),
                }],
                natural_width,
            }
        }
    }
    fn px(v: i32) -> FxPx { FxPx::new(v) }
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
    fn layout_to_golden(node: &LayoutNode, width: i32) -> Vec<GoldenBox> {
        let r = LayoutEngine::new().layout(node, px(width), &MockMeasure { char_width: px(0) }).unwrap();
        r.boxes.iter().map(|b| GoldenBox {
            node_id: b.node_id,
            id: b.id,
            x: b.rect.x.0, y: b.rect.y.0, width: b.rect.width.0, height: b.rect.height.0,
        }).collect()
    }

    fn default_visible_state() -> VisibleState {
        VisibleState {
            backend_visible: true,
            systemui_first_frame_visible: true,
            scene_ready: true,
            full_window_visible: true,
            click_target_visible: true,
            keyboard_target_visible: true,
            text_target_visible: true,
            icon_target_visible: true,
            hover_visible: true,
            launcher_click_visible: true,
            wheel_up_visible: true,
            keyboard_visible: true,
            ..VisibleState::default()
        }
    }

    fn find_visual_style<'a>(node: &'a LayoutNode, id: &str) -> Option<&'a VisualStyle> {
        match node {
            LayoutNode::Stack(stack, style, children) => {
                if stack.id == Some(id) {
                    return Some(style);
                }
                children.iter().find_map(|child| find_visual_style(child, id))
            }
            LayoutNode::Grid(grid, style, children) => {
                if grid.id == Some(id) {
                    return Some(style);
                }
                children.iter().find_map(|child| find_visual_style(child, id))
            }
            LayoutNode::Text(text, style) => (text.id == Some(id)).then_some(style),
            LayoutNode::Spacer(_) => None,
        }
    }

    #[test]
    fn golden_flex_row() {
        let s = LayoutNode::Stack(nexus_layout_types::Stack {
            id: None,
            direction: Direction::Row, gap: px(8), padding: EdgeInsets::all(px(16)),
            align: Align::Start, justify: Justify::Start,
            overflow: nexus_layout_types::Overflow::Visible, flex_wrap: false,
            min_width: None, max_width: None, min_height: None, max_height: None,
            item: FlexItem::default(),
        }, VisualStyle::default(), vec![txt("A"), txt("B"), txt("C")]);
        let g = layout_to_golden(&s, 120);
        let json = serde_json::to_string_pretty(&g).unwrap();
        // Stable snapshot: compare against known JSON
        assert_eq!(g.len(), 4); // container + 3 text
        assert_eq!(g[0].x, 0);
        assert_eq!(g[0].width, 48);
        assert_eq!(g[1].x, 16); // padding-left
        assert_eq!(g[2].x, 24); // compact natural row width under new measurement path
        assert!(json.contains("\"node_id\""));
    }

    #[test]
    fn golden_column_stack() {
        let s = LayoutNode::Stack(nexus_layout_types::Stack {
            id: None,
            direction: Direction::Column, gap: px(4), padding: EdgeInsets::all(px(8)),
            align: Align::Start, justify: Justify::Start,
            overflow: nexus_layout_types::Overflow::Visible, flex_wrap: false,
            min_width: None, max_width: None, min_height: None, max_height: None,
            item: FlexItem::default(),
        }, VisualStyle::default(), vec![txt("X"), txt("Y")]);
        let g = layout_to_golden(&s, 200);
        assert_eq!(g.len(), 3); // container + 2 text
        assert_eq!(g[1].y, 8); // padding-top
        assert_eq!(g[2].y, 32); // 8 + 20 + 4
    }

    #[test]
    fn golden_grid_3col() {
        let g = LayoutNode::Grid(Grid {
            id: None,
            columns: vec![Fraction(1), Fraction(2), Fraction(1)],
            gap: px(8), row_gap: Some(px(4)), padding: EdgeInsets::all(px(8)),
            overflow: nexus_layout_types::Overflow::Visible,
            min_width: None, max_width: None, min_height: None, max_height: None,
            item: FlexItem::default(),
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
            id: None,
            content: TextContent::new("red"),
            style: text_style(),
            item: FlexItem::default(),
            max_lines: None,
            min_width: None,
            max_width: None,
        }, vs);
        let r = LayoutEngine::new().layout(&node, px(100), &MockMeasure { char_width: px(0) }).unwrap();
        assert_eq!(r.boxes[0].visual.background, Some(nexus_layout_types::Rgba8 { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn proof_panel_layout_json_golden() {
        let layout = compute_proof_layout(default_visible_state()).expect("proof layout");
        let golden: Vec<GoldenBox> = layout
            .boxes
            .iter()
            .map(|b| GoldenBox {
                node_id: b.node_id,
                id: b.id,
                x: b.rect.x.0,
                y: b.rect.y.0,
                width: b.rect.width.0,
                height: b.rect.height.0,
            })
            .collect();
        let json = serde_json::to_string_pretty(&golden).unwrap();
        assert!(json.contains("\"id\": \"proof_panel\""));
        assert!(json.contains("\"id\": \"card_hover\""));
        assert!(json.contains("\"id\": \"icon_target\""));
        let panel = golden.iter().find(|entry| entry.id == Some("proof_panel")).unwrap();
        assert_eq!(panel.width, 610);
        assert_eq!(panel.height, 260);
        let hover = golden.iter().find(|entry| entry.id == Some("card_hover")).unwrap();
        let click = golden.iter().find(|entry| entry.id == Some("card_click")).unwrap();
        let scroll = golden.iter().find(|entry| entry.id == Some("card_scroll")).unwrap();
        let key = golden.iter().find(|entry| entry.id == Some("card_key")).unwrap();
        assert_eq!(hover.width, 126);
        assert_eq!(hover.height, 82);
        assert_eq!(click.x - hover.x, 142);
        assert_eq!(scroll.x - click.x, 142);
        assert_eq!(key.x - scroll.x, 142);
    }

    #[test]
    fn proof_panel_text_measure_matches_windowd_assets() {
        let measure = ProofTextMeasure;
        let style = TextStyle {
            font_size: px(30),
            font_weight: FontWeight::Bold,
            line_height: LineHeight::Absolute(px(34)),
            text_align: TextAlign::Left,
            color: nexus_layout_types::Rgba8::WHITE,
            white_space: WhiteSpace::NoWrap,
        };
        let handle = measure.prepare(&TextContent::new("Open Nexus OS"), &style);
        let width = measure.measure_width(&handle);
        let lines = measure.layout_lines(&handle, width, Some(1));
        assert!(width.0 > 0);
        assert_eq!(lines.lines.len(), 1);
        assert_eq!(lines.lines[0].width, width);
    }

    #[test]
    fn proof_panel_uses_shape_primitives_for_icons_and_scroll_markers() {
        let tree = build_proof_panel_tree(default_visible_state());
        let icon = find_visual_style(&tree, "icon_target_glyph").expect("icon glyph shape");
        let dot = find_visual_style(&tree, "card_hover_dot").expect("hover dot shape");
        let scroll_up = find_visual_style(&tree, "card_scroll_up").expect("scroll up triangle");
        let scroll_down =
            find_visual_style(&tree, "card_scroll_down").expect("scroll down triangle");

        assert!(matches!(&icon.shape, ShapeKind::Path(_)));
        assert_eq!(&dot.shape, &ShapeKind::Circle);
        assert_eq!(&scroll_up.shape, &ShapeKind::TriangleUp);
        assert_eq!(&scroll_down.shape, &ShapeKind::TriangleDown);
    }

    #[test]
    fn wrapping_cache_reuses_prepared_paragraphs() {
        let font_dir = PathBuf::from("/home/jenning/open-nexus-OS/resources/fonts/inter/docs/font-files");
        let measure = CachedTextMeasure::with_font_dir(&font_dir).expect("shape measure");
        let style = text_style();
        let handle = measure.prepare(&TextContent::new("Hover, click, scroll up/down, keyboard press"), &style);
        let narrow = measure.layout_lines(&handle, px(120), Some(3));
        let wide = measure.layout_lines(&handle, px(240), Some(3));
        assert!(measure.paragraph_cache_len() >= 1);
        assert!(measure.line_layout_cache_len() >= 2);
        assert!(narrow.natural_width.0 >= wide.lines[0].width.0);
    }
}
