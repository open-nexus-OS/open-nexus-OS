// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for the deterministic layout engine covering empty layouts, text
//! measurement, column/row flex layout, grid layout, node limits, div-by-zero edge cases,
//! visual style propagation, and shrink-respecting flex children.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_SCOPE: Layout engine (flex, grid, text, visual styles)
//! TEST_SCENARIOS: 10 tests (empty, text, col, row, max_nodes, grid, grid_div0,
//! visual_style_propagated, column_shrink_respects_zero_shrink_children,
//! vertical_scroll_viewport_width_never_squeezes_siblings)
//! ADR: docs/adr/0030-layout-engine-deterministic-pretext.md

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

    /// A vertical scroll viewport's CONTENT width must not leak into the
    /// parent flex negotiation (settings: the overview grid measured wider
    /// than the window through the content pane's scroller and the row
    /// deficit squeezed the fixed sidebar). The viewport measures width 0 and
    /// a column parent stretches it to the pane width at placement.
    #[test]
    fn vertical_scroll_viewport_width_never_squeezes_siblings() {
        let wide_text = LayoutNode::Text(
            TextNode {
                id: Some("wide"),
                content: TextContent::new("this content is far wider than the row"),
                style: text_style(),
                item: FlexItem::default(),
                max_lines: None,
                min_width: None,
                max_width: None,
            },
            VisualStyle::default(),
        );
        let scroller = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("scroller"),
                direction: Direction::Column,
                gap: px(0),
                padding: EdgeInsets::all(px(0)),
                align: Align::Start,
                justify: Justify::Start,
                overflow: nexus_layout_types::Overflow::Scroll(
                    nexus_layout_types::ScrollAxis::Vertical,
                ),
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
            },
            VisualStyle::default(),
            vec![wide_text],
        );
        let pane = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("pane"),
                direction: Direction::Column,
                gap: px(0),
                padding: EdgeInsets::all(px(0)),
                align: Align::Start,
                justify: Justify::Start,
                overflow: nexus_layout_types::Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
            },
            VisualStyle::default(),
            vec![scroller],
        );
        let sidebar = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("sidebar"),
                direction: Direction::Column,
                gap: px(0),
                padding: EdgeInsets::all(px(0)),
                align: Align::Start,
                justify: Justify::Start,
                overflow: nexus_layout_types::Overflow::Visible,
                flex_wrap: false,
                min_width: Some(px(80)),
                max_width: Some(px(80)),
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            vec![],
        );
        let root = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("root"),
                direction: Direction::Row,
                gap: px(0),
                padding: EdgeInsets::all(px(0)),
                align: Align::Stretch,
                justify: Justify::Start,
                overflow: nexus_layout_types::Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: Some(px(100)),
                max_height: Some(px(100)),
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            vec![sidebar, pane],
        );
        // 39 chars * 10px = 390px of content in a 200px row.
        let r = LayoutEngine::new()
            .layout(&root, px(200), &MockMeasure { char_width: px(10) })
            .unwrap();
        let sidebar = r.boxes.iter().find(|b| b.id == Some("sidebar")).unwrap();
        let pane = r.boxes.iter().find(|b| b.id == Some("pane")).unwrap();
        let scroller = r.boxes.iter().find(|b| b.id == Some("scroller")).unwrap();
        // The fixed sidebar keeps its width; the pane takes exactly the rest.
        assert_eq!(sidebar.rect.width, px(80));
        assert_eq!(pane.rect.x, px(80));
        assert_eq!(pane.rect.width, px(120));
        // The scroll viewport fills the pane (block fill-available), it does
        // not collapse to its measured 0 nor balloon to its 390px content.
        assert_eq!(scroller.rect.width, px(120));
    }

    // ---------------------------------------------------------------- basis
    // `.basis(n)` — the flex BASE SIZE. Without it `.grow` only shares out the
    // LEFTOVER on top of each child's own measured width, so a keypad row of
    // `AC`/`7`/`8`/`9` is never evenly divided. Every test below asserts the
    // fix AND (via `keys_without_basis_are_uneven`) that the bug was real.

    /// A child wrapped so it carries its own `FlexItem` — the shape the DSL
    /// produces for `Stack { … }.grow(n).basis(m)`.
    fn flex_child(id: &'static str, label: &str, grow: u32, basis: Option<i32>) -> LayoutNode {
        LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some(id),
                direction: Direction::Row,
                gap: px(0),
                padding: EdgeInsets::all(px(0)),
                align: Align::Stretch,
                justify: Justify::Start,
                overflow: nexus_layout_types::Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem {
                    flex_grow: grow,
                    flex_basis: basis.map(px),
                    ..FlexItem::default()
                },
            },
            VisualStyle::default(),
            vec![txt(label)],
        )
    }

    fn keypad_row(children: Vec<LayoutNode>, gap: i32) -> LayoutNode {
        LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("row"),
                direction: Direction::Row,
                gap: px(gap),
                padding: EdgeInsets::all(px(0)),
                align: Align::Stretch,
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
            children,
        )
    }

    fn width_of(r: &crate::LayoutResult, id: &str) -> FxPx {
        r.boxes.iter().find(|b| b.id == Some(id)).unwrap().rect.width
    }

    /// The bug `.basis` exists to fix: equal `.grow(1)` does NOT equalise
    /// children whose measured widths differ.
    #[test]
    fn keys_without_basis_are_uneven() {
        let row = keypad_row(
            vec![
                flex_child("k0", "AC", 1, None),
                flex_child("k1", "7", 1, None),
                flex_child("k2", "8", 1, None),
                flex_child("k3", "9", 1, None),
            ],
            10,
        );
        let r =
            LayoutEngine::new().layout(&row, px(400), &MockMeasure { char_width: px(10) }).unwrap();
        assert_ne!(
            width_of(&r, "k0"),
            width_of(&r, "k1"),
            "without basis the two-character key must stay wider — otherwise this \
             whole primitive is unnecessary and the test is lying"
        );
    }

    #[test]
    fn row_basis_zero_divides_exactly() {
        for container in [200, 400, 823] {
            let row = keypad_row(
                vec![
                    flex_child("k0", "AC", 1, Some(0)),
                    flex_child("k1", "7", 1, Some(0)),
                    flex_child("k2", "8", 1, Some(0)),
                    flex_child("k3", "9", 1, Some(0)),
                ],
                10,
            );
            let r = LayoutEngine::new()
                .layout(&row, px(container), &MockMeasure { char_width: px(10) })
                .unwrap();
            let widths: Vec<i32> =
                ["k0", "k1", "k2", "k3"].iter().map(|id| width_of(&r, id).0).collect();
            // Equal to the pixel where the space divides, otherwise within one
            // — the same guarantee `place_grid` gives its 1fr column tracks.
            let (lo, hi) = (*widths.iter().min().unwrap(), *widths.iter().max().unwrap());
            assert!(hi - lo <= 1, "cells uneven at container {container}: {widths:?}");
            // …and the row TILES: no lost pixels on the trailing edge.
            assert_eq!(
                widths.iter().sum::<i32>() + 30,
                container,
                "cells+gaps must tile the row at {container}: {widths:?}"
            );
            // The whole point: this is dramatically tighter than no basis.
            assert!(hi - lo < 5, "basis must beat the ~10px label-width spread");
        }
    }

    /// The `0` key: two tracks plus the gap between them, without Grid spans.
    #[test]
    fn basis_plus_grow_spans_a_track() {
        let row = keypad_row(
            vec![
                flex_child("zero", "0", 2, Some(10)),
                flex_child("comma", ",", 1, Some(0)),
                flex_child("eq", "=", 1, Some(0)),
            ],
            10,
        );
        let r =
            LayoutEngine::new().layout(&row, px(360), &MockMeasure { char_width: px(10) }).unwrap();
        let (zero, comma, eq) =
            (width_of(&r, "zero").0, width_of(&r, "comma").0, width_of(&r, "eq").0);
        assert!((comma - eq).abs() <= 1, "single-track cells {comma}/{eq} must match within 1px");
        // Spanning two tracks means covering both cells AND the gap they
        // straddle (±1 for the same indivisible-remainder reason as above).
        assert!((zero - (comma * 2 + 10)).abs() <= 1, "zero={zero} should span 2*{comma}+10");
        // And the row still tiles exactly.
        assert_eq!(zero + comma + eq + 20, 360);
    }

    /// The `Spacer` special case (`effective_item` overrides `flex_grow` from
    /// the spacer's own field): a basis-0 sibling must not disturb it.
    #[test]
    fn basis_coexists_with_spacer_grow() {
        let row = keypad_row(
            vec![
                LayoutNode::Spacer(nexus_layout_types::Spacer::default()),
                flex_child("tail", "9", 1, Some(0)),
            ],
            0,
        );
        let r =
            LayoutEngine::new().layout(&row, px(200), &MockMeasure { char_width: px(10) }).unwrap();
        // Spacer grow 1 and tail grow 1, both from a 0 base → an even split.
        assert_eq!(width_of(&r, "tail"), px(100));
    }
}
