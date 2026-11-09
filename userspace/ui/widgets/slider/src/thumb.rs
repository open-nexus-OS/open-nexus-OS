// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The thumb slider (the design artifacts' default `Slider`): inside the 28 px hit track, a
//! 6 px track in `fill` from the thumb to the end, the tint fill (with its `0 0 8px` glow) from
//! the start to the thumb, and the 20 px white thumb (`onAccent`, `shadow1`) between them. The
//! split is by flex weights, so the thumb's centre runs from 10 px to width − 10 px; the
//! optional ticks (2 × 5, `labelTertiary`) sit 22 px down on the same travel.

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, BoxShadow, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Position, Stack,
    VisualStyle,
};
use nexus_style::control::glow;
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, RadiusToken, ShadowToken, Tokens};

use crate::Slider;

/// The thin track's height.
const RAIL: i32 = 6;
/// The thumb's diameter.
const THUMB: i32 = 20;
/// The ticks' size and their distance from the hit track's top.
const TICK_W: i32 = 2;
const TICK_H: i32 = 5;
const TICK_TOP: i32 = 22;

/// A share of the travel carrying a 6 px rail in `visual`.
fn rail_part(weight: u32, visual: VisualStyle) -> LayoutNode {
    let rail = LayoutNode::Stack(
        Stack {
            min_height: Some(FxPx::new(RAIL)),
            max_height: Some(FxPx::new(RAIL)),
            item: FlexItem { flex_grow: 1, ..FlexItem::default() },
            ..Stack::default()
        },
        visual,
        Vec::new(),
    );
    LayoutNode::Stack(
        Stack {
            direction: Direction::Row,
            align: Align::Center,
            item: FlexItem { flex_grow: weight, flex_shrink: 1, ..FlexItem::default() },
            ..Stack::default()
        },
        VisualStyle::default(),
        alloc::vec![rail],
    )
}

impl Slider {
    fn thumb_node(tokens: &dyn Tokens) -> LayoutNode {
        let d = Some(FxPx::new(THUMB));
        LayoutNode::Stack(
            Stack {
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            // White in both themes (the artifacts' knob), lifted by `shadow1`.
            Style::new()
                .background_token(tokens, ColorToken::OnAccent)
                .rounded_token(RadiusToken::Full)
                .shadow_token(tokens, ShadowToken::Shadow1)
                .visual(),
            Vec::new(),
        )
    }

    /// `count` tick marks on the thumb's travel, below the track (an overlay of the track).
    fn tick_row(count: u8, tokens: &dyn Tokens) -> Option<LayoutNode> {
        if count < 2 {
            return None;
        }
        let tick = || {
            LayoutNode::Stack(
                Stack {
                    min_width: Some(FxPx::new(TICK_W)),
                    max_width: Some(FxPx::new(TICK_W)),
                    min_height: Some(FxPx::new(TICK_H)),
                    max_height: Some(FxPx::new(TICK_H)),
                    ..Stack::default()
                },
                Style::new()
                    .background_token(tokens, ColorToken::LabelTertiary)
                    .rounded_token(RadiusToken::Full)
                    .visual(),
                Vec::new(),
            )
        };
        let half = THUMB / 2 - TICK_W / 2;
        Some(LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(half)),
                align: Align::Start,
                justify: Justify::SpaceBetween,
                item: FlexItem {
                    position: Position::Absolute,
                    // An absolute child with a grow fills the track (the overlay contract).
                    flex_grow: 1,
                    margin: EdgeInsets { top: FxPx::new(TICK_TOP), ..EdgeInsets::zero() },
                    ..FlexItem::default()
                },
                ..Stack::default()
            },
            VisualStyle::default(),
            (0..count).map(|_| tick()).collect(),
        ))
    }

    pub(crate) fn thumb(self, tokens: &dyn Tokens) -> LayoutNode {
        let weight = self.fill_weight();
        let tint = self.tint.unwrap_or(ColorToken::Accent);
        let filled = VisualStyle {
            background: Some(tokens.color(tint)),
            shadow: Some(BoxShadow {
                offset_x: FxPx::ZERO,
                offset_y: FxPx::ZERO,
                blur_radius: FxPx::new(8),
                spread: FxPx::ZERO,
                color: glow(tokens, tint),
            }),
            ..Style::new().rounded_token(RadiusToken::Full).visual()
        };
        let base = Style::new()
            .background_token(tokens, ColorToken::Fill)
            .rounded_token(RadiusToken::Full)
            .visual();
        let mut children = alloc::vec![
            rail_part(weight, filled),
            Self::thumb_node(tokens),
            rail_part(100 - weight, base),
        ];
        children.extend(Self::tick_row(self.ticks, tokens));
        let mut visual = Style::new().rounded_token(RadiusToken::Full).visual();
        if self.state.shows_focus_ring() {
            visual = Style::new()
                .rounded_token(RadiusToken::Full)
                .focus_ring(tokens, ColorToken::Accent)
                .visual();
        }
        self.dim(&mut visual);
        let mut node = self.track(visual, children);
        if let LayoutNode::Stack(stack, _, _) = &mut node {
            stack.min_width = Some(FxPx::new(40));
        }
        node
    }
}
