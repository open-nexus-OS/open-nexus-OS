// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for the layout's scroll roots (RFC-0109): a top-level scroll container
//! is the root of its content — every box of it carries the root, its clips are only those inside
//! the content (the compositor clips it to the viewport where it scrolls), a scroll container
//! nested in another's content clips like `Hidden` and stays its outer root's; a scroller's size
//! follows its scroll axis, and a horizontal scroller keeps its children's widths.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_SCOPE: Layout engine (scroll roots, content clips, scroll measure)

#[cfg(test)]
mod tests {
    use crate::engine::LayoutEngine;
    use crate::LayoutBox;
    use nexus_layout_types::{
        Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, LineHeight,
        LineLayout, LineMetrics, MeasureText, Overflow, PreparedTextHandle, Rect, ScrollAxis,
        Stack, TextAlign, TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
    };

    struct Measure;
    impl MeasureText for Measure {
        fn prepare(&self, content: &TextContent, _: &TextStyle) -> PreparedTextHandle {
            PreparedTextHandle(content.as_str().chars().count())
        }
        fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx {
            FxPx::new(10 * handle.0 as i32)
        }
        fn layout_lines(
            &self,
            handle: &PreparedTextHandle,
            width: FxPx,
            _: Option<u32>,
        ) -> LineLayout {
            let natural_width = self.measure_width(handle);
            LineLayout {
                lines: vec![LineMetrics {
                    text_range: 0..handle.0,
                    width: natural_width.min(width),
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

    fn text(id: &'static str) -> LayoutNode {
        LayoutNode::Text(
            TextNode {
                id: Some(id),
                content: TextContent::new("row"),
                style: TextStyle {
                    font_size: px(16),
                    font_weight: FontWeight::Normal,
                    line_height: LineHeight::Absolute(px(20)),
                    text_align: TextAlign::Left,
                    color: nexus_layout_types::Rgba8::WHITE,
                    white_space: WhiteSpace::Normal,
                    tracking_milli_em: 0,
                    tabular: false,
                },
                item: FlexItem::default(),
                max_lines: None,
                min_width: None,
                max_width: None,
            },
            VisualStyle::default(),
        )
    }

    fn stack(
        id: &'static str,
        direction: Direction,
        overflow: Overflow,
        item: FlexItem,
        size: (Option<i32>, Option<i32>),
        children: Vec<LayoutNode>,
    ) -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                id: Some(id),
                direction,
                gap: px(0),
                padding: EdgeInsets::all(px(0)),
                align: Align::Start,
                justify: Justify::Start,
                overflow,
                flex_wrap: false,
                min_width: size.0.map(px),
                max_width: size.0.map(px),
                min_height: size.1.map(px),
                max_height: size.1.map(px),
                item,
                text_fit: None,
            },
            VisualStyle::default(),
            children,
        )
    }

    fn grow() -> FlexItem {
        FlexItem { flex_grow: 1, ..FlexItem::default() }
    }

    fn by_id<'a>(boxes: &'a [LayoutBox], id: &str) -> &'a LayoutBox {
        boxes.iter().find(|b| b.id == Some(id)).expect(id)
    }

    /// A 200-row page: a 60-row header, then a scroller taking the rest, its twenty 20-row rows.
    fn page(content: Vec<LayoutNode>) -> Vec<LayoutBox> {
        let scroller = stack(
            "scroller",
            Direction::Column,
            Overflow::Scroll(ScrollAxis::Vertical),
            grow(),
            (None, None),
            content,
        );
        let header = stack(
            "header",
            Direction::Column,
            Overflow::Visible,
            FlexItem::default(),
            (None, Some(60)),
            vec![],
        );
        let root = stack(
            "root",
            Direction::Column,
            Overflow::Visible,
            FlexItem::default(),
            (None, None),
            vec![header, scroller],
        );
        LayoutEngine::new()
            .layout_with_viewport(&root, px(300), Some(px(200)), &Measure)
            .expect("lays out")
            .boxes
    }

    #[test]
    fn a_scroll_containers_content_carries_its_root_and_no_viewport_clip() {
        let rows = (0..20).map(|_| text("row")).collect();
        let boxes = page(rows);
        let scroller = by_id(&boxes, "scroller");
        assert_eq!(scroller.scroll_root, None, "the container is its parent's (the base)");
        assert_eq!(
            scroller.content_clip,
            Some(Rect::new(px(0), px(60), px(300), px(140))),
            "its viewport"
        );
        assert_eq!(scroller.clip_rect, None, "its own paint: its ancestors' clip");
        let rows: Vec<&LayoutBox> = boxes.iter().filter(|b| b.id == Some("row")).collect();
        assert_eq!(rows.len(), 20);
        for r in &rows {
            assert_eq!(r.scroll_root, Some(scroller.node_id));
            assert_eq!(r.clip_rect, None, "the viewport clips in the compositor, not here");
        }
        // The last row lies far below the viewport, at identity.
        assert_eq!(rows[19].rect.y, px(60 + 19 * 20));
        assert_eq!(by_id(&boxes, "header").scroll_root, None);
    }

    /// A clip INSIDE the content stays the content's own: a card below the viewport still clips
    /// its child to the card, not to nothing.
    #[test]
    fn a_clip_inside_the_content_is_the_contents_own() {
        let mut content: Vec<LayoutNode> = (0..10).map(|_| text("row")).collect();
        let card = stack(
            "card",
            Direction::Column,
            Overflow::Hidden,
            FlexItem::default(),
            (Some(200), Some(40)),
            vec![text("inside")],
        );
        content.push(card);
        let boxes = page(content);
        let card = by_id(&boxes, "card");
        assert_eq!(card.rect.y, px(60 + 10 * 20), "below the 140-row viewport");
        let inside = by_id(&boxes, "inside");
        assert_eq!(inside.clip_rect, Some(Rect::new(px(0), px(260), px(200), px(40))));
        assert_eq!(inside.scroll_root, card.scroll_root);
    }

    /// One scroll container nested in another's content is no second root: it clips like
    /// `Hidden`, and its content stays its outer root's.
    #[test]
    fn a_nested_scroll_container_clips_and_stays_in_its_outer_root() {
        let inner = stack(
            "inner",
            Direction::Column,
            Overflow::Scroll(ScrollAxis::Vertical),
            FlexItem::default(),
            (Some(100), Some(40)),
            vec![text("deep"), text("deep"), text("deep")],
        );
        let boxes = page(vec![text("row"), inner]);
        let root = by_id(&boxes, "scroller").node_id;
        let inner = by_id(&boxes, "inner");
        assert_eq!(inner.scroll_root, Some(root));
        let deep: Vec<&LayoutBox> = boxes.iter().filter(|b| b.id == Some("deep")).collect();
        for d in deep {
            assert_eq!(d.scroll_root, Some(root), "the outer root's content");
            assert_eq!(d.clip_rect, inner.content_clip, "clipped to the inner viewport");
        }
    }

    /// A clipping container clips its CONTENT to its padding box; its own fill and padding are
    /// clipped only by its ancestors (a card's padding ring is painted and takes taps).
    #[test]
    fn a_clipping_containers_padding_is_its_own() {
        let mut card = stack(
            "card",
            Direction::Column,
            Overflow::Hidden,
            FlexItem::default(),
            (Some(200), Some(80)),
            vec![text("inside")],
        );
        if let LayoutNode::Stack(s, _, _) = &mut card {
            s.padding = EdgeInsets::all(px(8));
        }
        let root = stack(
            "root",
            Direction::Column,
            Overflow::Visible,
            FlexItem::default(),
            (None, None),
            vec![card],
        );
        let boxes = LayoutEngine::new()
            .layout_with_viewport(&root, px(300), Some(px(200)), &Measure)
            .expect("lays out")
            .boxes;
        let card = by_id(&boxes, "card");
        assert_eq!(card.clip_rect, None, "nothing above it clips");
        assert_eq!(card.content_clip, Some(Rect::new(px(8), px(8), px(184), px(64))));
        assert_eq!(by_id(&boxes, "inside").clip_rect, card.content_clip);
    }

    /// A horizontal (paged) scroller gives up its WIDTH and keeps its height; its page cells keep
    /// their widths instead of shrinking into the viewport (a pager squeezed its pages).
    #[test]
    fn a_horizontal_scroller_keeps_its_height_and_its_cells() {
        let cell = |id| {
            stack(
                id,
                Direction::Column,
                Overflow::Visible,
                FlexItem::default(),
                (Some(300), Some(50)),
                vec![],
            )
        };
        let pager = stack(
            "pager",
            Direction::Row,
            Overflow::Scroll(ScrollAxis::Paged),
            grow(),
            (None, None),
            vec![cell("p0"), cell("p1"), cell("p2")],
        );
        let root = stack(
            "root",
            Direction::Column,
            Overflow::Visible,
            FlexItem::default(),
            (None, None),
            vec![pager],
        );
        let boxes = LayoutEngine::new()
            .layout_with_viewport(&root, px(300), Some(px(200)), &Measure)
            .expect("lays out")
            .boxes;
        let p2 = by_id(&boxes, "p2");
        assert_eq!(p2.rect.width, px(300), "never shrunk");
        assert_eq!(p2.rect.x, px(600), "the third page beside the viewport");
        assert_eq!(by_id(&boxes, "p0").scroll_root, Some(by_id(&boxes, "pager").node_id));
    }

    /// A paged scroller's page without a width of its own is the viewport's width — the host
    /// snaps by whole viewports — and lays its content out against it: a 300-wide grid centred in
    /// a 500-wide page (the launcher's pages on a screen wider than their grid). A pinned page
    /// keeps its width (the test above).
    #[test]
    fn a_page_without_a_width_is_the_viewport_and_centres_its_content() {
        let page = |id, grid| {
            let cells = (Some(300), Some(50));
            let grid = stack(
                grid,
                Direction::Column,
                Overflow::Visible,
                FlexItem::default(),
                cells,
                vec![],
            );
            let mut page = stack(
                id,
                Direction::Column,
                Overflow::Visible,
                FlexItem::default(),
                (None, None),
                vec![grid],
            );
            if let LayoutNode::Stack(s, ..) = &mut page {
                s.align = Align::Center;
            }
            page
        };
        let pager = stack(
            "pager",
            Direction::Row,
            Overflow::Scroll(ScrollAxis::Paged),
            grow(),
            (None, None),
            vec![page("p0", "g0"), page("p1", "g1")],
        );
        let root = stack(
            "root",
            Direction::Column,
            Overflow::Visible,
            FlexItem::default(),
            (None, None),
            vec![pager],
        );
        let boxes = LayoutEngine::new()
            .layout_with_viewport(&root, px(500), Some(px(200)), &Measure)
            .expect("lays out")
            .boxes;
        assert_eq!(by_id(&boxes, "p0").rect.width, px(500), "a page is the viewport");
        assert_eq!(by_id(&boxes, "p1").rect.x, px(500), "the next page one viewport on");
        assert_eq!(by_id(&boxes, "g0").rect.x, px(100), "the grid centred in its page");
        assert_eq!(by_id(&boxes, "g1").rect.x, px(600));
    }

    /// A column stretches its children to its width — within each child's own bounds: a pinned
    /// `.width(340)` keeps 340 at the cross start, a `.maxWidth` caps the stretch (a stretched
    /// login column and the launcher's search bar took the whole column before).
    #[test]
    fn a_stretched_child_keeps_its_own_width_bounds() {
        let pinned = stack(
            "pinned",
            Direction::Column,
            Overflow::Visible,
            FlexItem::default(),
            (Some(340), Some(20)),
            vec![],
        );
        let mut capped = stack(
            "capped",
            Direction::Column,
            Overflow::Visible,
            FlexItem::default(),
            (None, Some(20)),
            vec![],
        );
        if let LayoutNode::Stack(s, ..) = &mut capped {
            s.max_width = Some(px(520));
        }
        let free = stack(
            "free",
            Direction::Column,
            Overflow::Visible,
            FlexItem::default(),
            (None, Some(20)),
            vec![],
        );
        let mut root = stack(
            "root",
            Direction::Column,
            Overflow::Visible,
            FlexItem::default(),
            (None, None),
            vec![pinned, capped, free],
        );
        if let LayoutNode::Stack(s, ..) = &mut root {
            s.align = Align::Stretch;
        }
        let boxes = LayoutEngine::new()
            .layout_with_viewport(&root, px(1184), Some(px(200)), &Measure)
            .expect("lays out")
            .boxes;
        assert_eq!(by_id(&boxes, "pinned").rect.width, px(340));
        assert_eq!(by_id(&boxes, "pinned").rect.x, px(0), "at the cross start");
        assert_eq!(by_id(&boxes, "capped").rect.width, px(520));
        assert_eq!(by_id(&boxes, "free").rect.width, px(1184), "an unbounded child stretches");

        // The same in a row's cross axis: a stretched `.height(28)` keeps 28, at the top.
        let pill = stack(
            "pill",
            Direction::Row,
            Overflow::Visible,
            FlexItem::default(),
            (Some(80), Some(28)),
            vec![],
        );
        let tall = stack(
            "tall",
            Direction::Row,
            Overflow::Visible,
            FlexItem::default(),
            (Some(80), None),
            vec![],
        );
        let mut bar = stack(
            "bar",
            Direction::Row,
            Overflow::Visible,
            FlexItem::default(),
            (None, Some(36)),
            vec![pill, tall],
        );
        if let LayoutNode::Stack(s, ..) = &mut bar {
            s.align = Align::Stretch;
        }
        let boxes = LayoutEngine::new()
            .layout_with_viewport(&bar, px(400), Some(px(36)), &Measure)
            .expect("lays out")
            .boxes;
        assert_eq!(by_id(&boxes, "pill").rect.height, px(28));
        assert_eq!(by_id(&boxes, "pill").rect.y, px(0));
        assert_eq!(by_id(&boxes, "tall").rect.height, px(36), "an unbounded child stretches");
    }
}
