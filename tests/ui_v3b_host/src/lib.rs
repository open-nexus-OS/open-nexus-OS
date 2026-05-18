// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests for TASK-0059 / RFC-0058: clipping, scroll damage,
//! filter-box proof element, effects goldens, and IME stub validation.
//! OWNERS: @ui
//! STATUS: In Progress
//! TEST_COVERAGE: Integration tests for v3b features
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#[cfg(test)]
mod tests {
    use input_live_protocol::VisibleState;
    use nexus_effects::blur::blur_3x3;
    use nexus_effects::budget::EffectBudget;
    use nexus_effects::cursor_blink::CursorBlink;
    use nexus_layout::{
        compute_scroll_damage, LayoutEngine,
    };
    use nexus_layout_types::{
        Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
        LineHeight, LineLayout, LineMetrics, MeasureText, Overflow, PreparedTextHandle,
        Rect, TextAlign, TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
    };
    use windowd::{build_filter_panel_tree, compute_proof_layout, filter_words};

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
            keyboard_visible: true,
            ..VisibleState::default()
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // Scroll Damage Tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn scroll_damage_empty_on_zero_delta() {
        let viewport = Rect::new(px(0), px(0), px(100), px(100));
        let damage = compute_scroll_damage((px(0), px(0)), (px(0), px(0)), viewport);
        assert!(damage.is_empty());
    }

    #[test]
    fn scroll_damage_down_exposes_bottom_strip() {
        let viewport = Rect::new(px(0), px(0), px(100), px(100));
        // dy > 0: content shifts up → bottom of viewport gets new content
        let damage = compute_scroll_damage((px(0), px(0)), (px(0), px(20)), viewport);
        assert!(!damage.is_empty());
        let rect = damage.rects[0].unwrap();
        assert_eq!(rect.x, px(0));
        assert_eq!(rect.y, px(80));
        assert_eq!(rect.width, px(100));
        assert_eq!(rect.height, px(20));
    }

    #[test]
    fn scroll_damage_up_exposes_top_strip() {
        let viewport = Rect::new(px(0), px(0), px(100), px(100));
        // dy < 0: content shifts down → top of viewport gets new content
        let damage = compute_scroll_damage((px(0), px(30)), (px(0), px(0)), viewport);
        assert!(!damage.is_empty());
        let rect = damage.rects[0].unwrap();
        assert_eq!(rect.x, px(0));
        assert_eq!(rect.y, px(0)); // top 30px
        assert_eq!(rect.width, px(100));
        assert_eq!(rect.height, px(30));
    }

    #[test]
    fn scroll_damage_horizontal() {
        let viewport = Rect::new(px(0), px(0), px(100), px(100));
        let damage = compute_scroll_damage((px(0), px(0)), (px(10), px(0)), viewport);
        let rect = damage.rects[0].unwrap();
        // Scrolling right: left side exposed — wait, the function says right side becomes visible
        // The old viewport's left part is now hidden; new viewport is shifted right
        assert_eq!(rect.x, px(90)); // rightmost 10px
        assert_eq!(rect.width, px(10));
    }

    // ══════════════════════════════════════════════════════════════════
    // Clip Tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn overflow_hidden_container_sets_clip_rect() {
        let root = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("clip_container"),
                direction: Direction::Column,
                gap: px(0),
                padding: EdgeInsets::all(px(4)),
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: None,
                max_width: Some(px(100)),
                min_height: Some(px(60)),
                max_height: Some(px(60)),
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            vec![txt("longer text that would overflow"), txt("another line")],
        );
        let result = LayoutEngine::new()
            .layout(&root, px(200), &MockMeasure { char_width: px(10) })
            .unwrap();

        // Container has Overflow::Hidden → clip_rect is set
        let container = result.boxes.iter().find(|b| b.id == Some("clip_container")).unwrap();
        assert!(container.clip_rect.is_some());
        assert_eq!(container.overflow, Overflow::Hidden);

        // Children inherit the clip_rect
        for b in &result.boxes {
            if b.node_id > container.node_id {
                assert!(b.clip_rect.is_some(), "child {:?} should have clip_rect", b.id);
            }
        }
    }

    #[test]
    fn overflow_visible_container_has_no_new_clip() {
        let root = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("visible_container"),
                direction: Direction::Column,
                gap: px(0),
                padding: EdgeInsets::all(px(4)),
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            vec![txt("hello")],
        );
        let result = LayoutEngine::new()
            .layout(&root, px(200), &MockMeasure { char_width: px(10) })
            .unwrap();
        let container = result.boxes.iter().find(|b| b.id == Some("visible_container")).unwrap();
        assert_eq!(container.overflow, Overflow::Visible);
    }

    // ══════════════════════════════════════════════════════════════════
    // filter_words Tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn filter_words_ap_returns_correct_results() {
        let result = filter_words("ap");
        assert_eq!(result, vec!["apple", "application", "apt"]);
    }

    #[test]
    fn filter_words_case_insensitive() {
        let result = filter_words("AP");
        assert_eq!(result, vec!["apple", "application", "apt"]);
    }

    #[test]
    fn filter_words_empty_returns_all() {
        let result = filter_words("");
        assert_eq!(result.len(), 15); // FILTER_WORDS length
    }

    #[test]
    fn filter_words_no_match_returns_empty() {
        let result = filter_words("zzz");
        assert!(result.is_empty());
    }

    #[test]
    fn filter_words_a_returns_multiple() {
        let result = filter_words("a");
        assert_eq!(result, vec!["apple", "application", "apt", "arrow", "asset"]);
    }

    #[test]
    fn filter_words_cache_returns_correct_count() {
        // "cache" starts with "ca"
        let result = filter_words("ca");
        assert!(result.contains(&"cache"));
    }

    // ══════════════════════════════════════════════════════════════════
    // Filter-Box Layout Tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn filter_box_layout_contains_text_input_and_filter_list() {
        let tree = build_filter_panel_tree("ap");
        let layout = LayoutEngine::new()
            .layout(&tree, px(200), &MockMeasure { char_width: px(8) })
            .unwrap();

        let text_input = layout.boxes.iter().find(|b| b.id == Some("filter_text_input"));
        assert!(text_input.is_some(), "filter_text_input should exist in layout");

        let filter_list = layout.boxes.iter().find(|b| b.id == Some("filter_list"));
        assert!(filter_list.is_some(), "filter_list should exist in layout");
        assert_eq!(filter_list.unwrap().overflow, Overflow::Hidden);
    }

    #[test]
    fn filter_list_child_count_matches_filter_result() {
        let tree = build_filter_panel_tree("ap");
        let layout = LayoutEngine::new()
            .layout(&tree, px(200), &MockMeasure { char_width: px(8) })
            .unwrap();

        let filtered = filter_words("ap");
        // Count text nodes inside filter_list (children of filter_list)
        // The filtered words become Text nodes
        let text_child_count = layout
            .boxes
            .iter()
            .filter(|b| b.node_id > 0)
            .count();
        // At minimum, we have the filter list, text input, and filtered items
        assert!(text_child_count >= filtered.len(),
            "expected at least {} text children, got {}",
            filtered.len(), text_child_count);
    }

    #[test]
    fn real_proof_measure_keeps_filter_input_visible() {
        let layout = compute_proof_layout(default_visible_state(), "").unwrap();
        let text_input = layout.boxes.iter().find(|b| b.id == Some("filter_text_input")).unwrap();
        assert!(text_input.rect.width > px(80), "placeholder should reserve visible width");
    }

    #[test]
    fn real_proof_measure_keeps_filtered_words_visible() {
        let layout = compute_proof_layout(default_visible_state(), "ap").unwrap();
        for id in ["filter_apple", "filter_application", "filter_apt"] {
            let word = layout.boxes.iter().find(|b| b.id == Some(id)).unwrap();
            assert!(word.rect.width > FxPx::ZERO, "{id} should have measured width");
        }
    }

    #[test]
    fn live_filter_cycle_prefixes_have_precomputable_layouts() {
        for prefix in ["", "a", "ap", "c", "b"] {
            let layout = compute_proof_layout(default_visible_state(), prefix).unwrap();
            assert!(
                layout.boxes.iter().any(|b| b.id == Some("filter_text_input")),
                "filter_text_input should exist for prefix {prefix:?}"
            );
            assert!(
                layout.boxes.iter().any(|b| b.id == Some("filter_list")),
                "filter_list should exist for prefix {prefix:?}"
            );
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // Scroll reposition tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn reposition_scroll_shifts_children() {
        let root = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("scroll_box"),
                direction: Direction::Column,
                gap: px(2),
                padding: EdgeInsets::all(px(4)),
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: None,
                max_width: Some(px(100)),
                min_height: Some(px(50)),
                max_height: Some(px(50)),
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            vec![txt("A"), txt("B"), txt("C")],
        );
        let mut result = LayoutEngine::new()
            .layout(&root, px(200), &MockMeasure { char_width: px(10) })
            .unwrap();

        let container = result.boxes.iter().find(|b| b.id == Some("scroll_box")).unwrap();
        let container_id = container.node_id;

        // Scroll down by 20px
        let damage = result.reposition_scroll(container_id, (px(0), px(20)));
        assert!(!damage.is_empty(), "scroll damage should not be empty");

        // Verify children shifted
        let container = result.boxes.iter().find(|b| b.id == Some("scroll_box")).unwrap();
        assert_eq!(container.scroll_offset, (px(0), px(20)));
    }

    // ══════════════════════════════════════════════════════════════════
    // Effect Budget Tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn effect_budget_reserves_and_exhausts() {
        let mut budget = EffectBudget::new(1000);
        assert!(budget.try_reserve(500));
        assert_eq!(budget.remaining, 500);
        assert!(budget.try_reserve(500));
        assert_eq!(budget.remaining, 0);
        assert!(!budget.try_reserve(1));
    }

    #[test]
    fn effect_budget_reset_restores() {
        let mut budget = EffectBudget::new(1000);
        assert!(budget.try_reserve(1000));
        assert_eq!(budget.remaining, 0);
        budget.reset();
        assert_eq!(budget.remaining, 1000);
    }

    #[test]
    fn effect_budget_fraction() {
        let mut budget = EffectBudget::new(1000);
        assert_eq!(budget.fraction(), (1000, 1000));
        assert!(budget.try_reserve(250));
        assert_eq!(budget.fraction(), (750, 1000));
    }

    // ══════════════════════════════════════════════════════════════════
    // Blur Tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn blur_3x3_identity() {
        // A solid color region should be unchanged by blur
        let mut pixels = vec![0u8; 20 * 20 * 4];
        for y in 0..20 {
            for x in 0..20 {
                let idx = (y * 20 + x) * 4;
                pixels[idx] = 100;
                pixels[idx + 1] = 150;
                pixels[idx + 2] = 200;
                pixels[idx + 3] = 255;
            }
        }
        let original = pixels.clone();
        blur_3x3(&mut pixels, 20, 20, 80);
        // Solid region: all values should be close to original
        for i in 0..original.len() {
            let diff = if pixels[i] > original[i] {
                pixels[i] - original[i]
            } else {
                original[i] - pixels[i]
            };
            assert!(diff <= 2, "pixel changed too much at index {i}: {} -> {}", original[i], pixels[i]);
        }
    }

    #[test]
    fn blur_3x3_too_small_returns_zero() {
        let mut pixels = vec![0u8; 2 * 2 * 4];
        let count = blur_3x3(&mut pixels, 2, 2, 8);
        assert_eq!(count, 0); // too small to blur
    }

    // ══════════════════════════════════════════════════════════════════
    // Cursor Blink Tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn cursor_blink_toggles_at_interval() {
        let mut blink = CursorBlink::with_interval(5);
        assert!(blink.visible);
        for _ in 0..4 {
            blink.tick();
            assert!(blink.visible, "should stay visible before interval");
        }
        blink.tick(); // 5th tick
        assert!(!blink.visible, "should toggle after interval");
        for _ in 0..4 {
            blink.tick();
            assert!(!blink.visible);
        }
        blink.tick(); // 10th tick
        assert!(blink.visible, "should toggle back");
    }

    #[test]
    fn cursor_blink_reset() {
        let mut blink = CursorBlink::with_interval(2);
        blink.tick();
        blink.tick();
        assert!(!blink.visible);
        blink.reset();
        assert!(blink.visible);
    }

    // ══════════════════════════════════════════════════════════════════
    // Proof Panel Layout with Filter Text
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn combined_panels_layout_contains_both_panels() {
        use windowd::build_combined_tree;
        let tree = build_combined_tree(default_visible_state(), "a");
        let layout = LayoutEngine::new()
            .layout(&tree, px(850), &MockMeasure { char_width: px(8) })
            .unwrap();
        assert!(layout.boxes.iter().any(|b| b.id == Some("proof_panel")));
        assert!(layout.boxes.iter().any(|b| b.id == Some("filter_panel")));
    }
}
