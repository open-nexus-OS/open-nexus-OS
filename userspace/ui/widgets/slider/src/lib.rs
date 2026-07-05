// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Slider` — the design-system range slider (handoff `Slider`): a glass track,
//! an accent-filled portion up to the value, and a white thumb. A pure builder
//! producing a `LayoutNode::Stack` row of [fill · thumb · remaining-track]. The
//! `id` is the interaction id; the app owns the value. DSL-emittable.

extern crate alloc;

use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    Rgba8, Stack, VisualStyle,
};
use nexus_style::InteractionState;
use nexus_theme_tokens::{ColorToken, Tokens};

const TRACK_W: i32 = 140;
const TRACK_H: i32 = 6;
const THUMB: i32 = 18;
/// Track length the thumb centre can travel across.
const USABLE: i32 = TRACK_W - THUMB;

/// A horizontal range slider (value 0..=100).
#[derive(Debug, Clone, Default)]
pub struct Slider {
    value: u8,
    state: InteractionState,
    id: Option<&'static str>,
}

impl Slider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Value as a percent 0..=100 (clamped).
    pub fn value(mut self, value: u8) -> Self {
        self.value = value.min(100);
        self
    }

    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Filled track width (px) for the current value.
    pub fn fill_width(&self) -> i32 {
        USABLE * self.value as i32 / 100
    }

    fn bar(w: i32, h: i32, color: Rgba8, radius: i32) -> LayoutNode {
        let visual = VisualStyle {
            background: Some(color),
            corner_radius: CornerRadius::uniform(FxPx::new(radius)),
            ..VisualStyle::default()
        };
        let (mw, mh) = (Some(FxPx::new(w)), Some(FxPx::new(h)));
        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: mw,
                max_width: mw,
                min_height: mh,
                max_height: mh,
                item: FlexItem::default(),
            },
            visual,
            alloc::vec![],
        )
    }

    /// Build the slider node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let fill_w = self.fill_width();
        let rest_w = USABLE - fill_w;
        let accent = tokens.color(ColorToken::Accent);
        let track = tokens.color(ColorToken::SurfaceVariant);
        // The thumb is the conventional white cap (theme-independent).
        let thumb_color = Rgba8::new(255, 255, 255, 255);

        let mut children = alloc::vec::Vec::new();
        if fill_w > 0 {
            children.push(Self::bar(fill_w, TRACK_H, accent, TRACK_H / 2));
        }
        children.push(Self::bar(THUMB, THUMB, thumb_color, THUMB / 2));
        if rest_w > 0 {
            children.push(Self::bar(rest_w, TRACK_H, track, TRACK_H / 2));
        }

        let opacity = self.state.is_disabled().then(|| self.state.opacity());
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: Some(FxPx::new(TRACK_W)),
                max_width: Some(FxPx::new(TRACK_W)),
                min_height: Some(FxPx::new(THUMB)),
                max_height: Some(FxPx::new(THUMB)),
                item: FlexItem::default(),
            },
            VisualStyle { opacity, ..VisualStyle::default() },
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn fill_tracks_value() {
        assert_eq!(Slider::new().value(0).fill_width(), 0);
        assert_eq!(Slider::new().value(100).fill_width(), USABLE);
        assert_eq!(Slider::new().value(200).value(50).fill_width(), USABLE / 2); // clamps
    }

    #[test]
    fn zero_value_has_no_fill_bar() {
        let t = BaseTokens;
        match Slider::new().value(0).build(&t) {
            // no fill (0) → thumb + rest = 2 children.
            LayoutNode::Stack(_, _, children) => assert_eq!(children.len(), 2),
            _ => panic!(),
        }
        match Slider::new().value(60).id("vol").build(&t) {
            LayoutNode::Stack(stack, _, children) => {
                assert_eq!(stack.id, Some("vol"));
                assert_eq!(children.len(), 3, "fill + thumb + rest");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn full_value_has_no_rest_bar() {
        let t = BaseTokens;
        match Slider::new().value(100).build(&t) {
            LayoutNode::Stack(_, _, children) => assert_eq!(children.len(), 2, "fill + thumb"),
            _ => panic!(),
        }
    }
}
