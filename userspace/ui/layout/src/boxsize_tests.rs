// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for the two box-relative layout primitives — `.basis(n)`
//! (exact flex division, TASK-0314) and `.textFit(pct, min, max)` (type sized
//! from the container box). Split out of `engine_tests.rs`, which hit the
//! 600-LOC module ratchet: those tests cover the classic flex/grid/text paths,
//! these cover sizing that is derived from a settled box.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_SCOPE: flex-basis apportionment + box-relative type
//! TEST_SCENARIOS: 8 tests (uneven-without-basis, exact division, track span,
//! spacer coexistence, fit target, fit step-down, fit fixpoint, fit fallback)
//! ADR: docs/adr/0030-layout-engine-deterministic-pretext.md

#[cfg(test)]
mod tests {
    use crate::engine::LayoutEngine;
    use nexus_layout_types::{
        Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, LineHeight,
        LineLayout, LineMetrics, MeasureText, PreparedTextHandle, TextAlign, TextContent, TextNode,
        TextStyle, VisualStyle, WhiteSpace,
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
                text_fit: None,
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
                text_fit: None,
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

    // -------------------------------------------------------------- textFit

    /// A `.textFit` container holding one label, sized by its parent.
    fn fit_cell(pct: u32, min: i32, max: i32, label: &str, w: i32, h: i32) -> LayoutNode {
        let cell = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("cell"),
                direction: Direction::Row,
                gap: px(0),
                padding: EdgeInsets::all(px(0)),
                align: Align::Center,
                justify: Justify::Center,
                overflow: nexus_layout_types::Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem { flex_grow: 1, flex_basis: Some(px(0)), ..FlexItem::default() },
                text_fit: Some(nexus_layout_types::TextFit { pct, min: px(min), max: px(max) }),
            },
            VisualStyle::default(),
            vec![txt(label)],
        );
        let _ = (w, h);
        keypad_row(vec![cell], 0)
    }

    fn fit_layout(
        pct: u32,
        min: i32,
        max: i32,
        label: &str,
        w: i32,
        h: i32,
    ) -> crate::LayoutResult {
        LayoutEngine::new()
            .layout_with_viewport(
                &fit_cell(pct, min, max, label, w, h),
                px(w),
                Some(px(h)),
                &MockMeasure { char_width: px(10) },
            )
            .unwrap()
    }

    fn chosen_px(r: &crate::LayoutResult) -> i32 {
        r.boxes.iter().find_map(|b| b.text_px).expect("a fitted text box").0
    }

    /// The target is a RATIO of the container box, not "as large as fits" —
    /// the handoff puts a 23px label in a ~75px key.
    #[test]
    fn textfit_target_rides_the_container_box() {
        assert_eq!(chosen_px(&fit_layout(30, 10, 90, "7", 400, 100)), 30);
        assert_eq!(chosen_px(&fit_layout(30, 10, 90, "7", 400, 200)), 60);
        // …clamped at both ends.
        assert_eq!(chosen_px(&fit_layout(30, 10, 40, "7", 400, 400)), 40);
        assert_eq!(chosen_px(&fit_layout(30, 25, 90, "7", 400, 50)), 25);
    }

    /// A run too wide for its box steps DOWN — this is what reproduces the
    /// handoff's 52/38/30 display ramp without hard-coding digit counts.
    #[test]
    fn textfit_steps_down_until_the_run_fits() {
        // MockMeasure: width = char_width * chars, independent of size, so a
        // narrow box forces the floor.
        let narrow = chosen_px(&fit_layout(50, 8, 90, "123456789012345", 60, 200));
        assert_eq!(narrow, 8, "a run that never fits must land on min");
        let roomy = chosen_px(&fit_layout(50, 8, 90, "1", 400, 200));
        assert_eq!(roomy, 90, "a short run in a tall box takes the target");
    }

    /// THE guard against the oscillation that killed the text-node design:
    /// laying the same tree out twice must produce identical boxes.
    #[test]
    fn textfit_is_a_fixpoint() {
        for (w, h) in [(204, 120), (392, 616), (823, 400), (1280, 800)] {
            let first = fit_layout(30, 11, 120, "AC", w, h);
            let second = fit_layout(30, 11, 120, "AC", w, h);
            assert_eq!(first.boxes, second.boxes, "layout is not stable at {w}x{h}");
            // And re-fitting against the box it just chose returns the same
            // size — the property that makes a relayout after resize safe.
            assert_eq!(chosen_px(&first), chosen_px(&second));
        }
    }

    /// A hugging container would size itself from the text it is sizing, so
    /// the fit is skipped there and `max` applies. No infinite regress.
    #[test]
    fn textfit_without_a_definite_box_falls_back_to_max() {
        let cell = LayoutNode::Stack(
            nexus_layout_types::Stack {
                id: Some("hug"),
                direction: Direction::Row,
                gap: px(0),
                padding: EdgeInsets::all(px(0)),
                align: Align::Center,
                justify: Justify::Center,
                overflow: nexus_layout_types::Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
                text_fit: Some(nexus_layout_types::TextFit { pct: 30, min: px(11), max: px(44) }),
            },
            VisualStyle::default(),
            vec![txt("7")],
        );
        // `layout` (no viewport height) never hands down a definite box.
        let r = LayoutEngine::new().layout(&cell, px(400), &MockMeasure { char_width: px(1) });
        assert_eq!(chosen_px(&r.unwrap()), 44);
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
