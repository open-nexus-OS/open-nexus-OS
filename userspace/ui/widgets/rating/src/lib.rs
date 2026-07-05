// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Rating` — the design-system star rating (handoff `Rating`): a row of star
//! [`Icon`]s, filled up to the value (warning tint) and muted beyond. A pure
//! builder producing a `LayoutNode::Stack` row; the `id` is the interaction id
//! (the app maps a tapped star index → value). DSL-emittable — and the first
//! consumer of the [`Icon`] symbol primitive.

extern crate alloc;

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack,
    VisualStyle,
};
use nexus_theme_tokens::{ColorToken, Tokens};
use nexus_widget_icon::{Icon, Symbol};

/// A star rating.
#[derive(Debug, Clone)]
pub struct Rating {
    value: u8,
    max: u8,
    size: i32,
    id: Option<&'static str>,
}

impl Default for Rating {
    fn default() -> Self {
        Self { value: 0, max: 5, size: 20, id: None }
    }
}

impl Rating {
    pub fn new() -> Self {
        Self::default()
    }

    /// Filled stars (clamped to `max`).
    pub fn value(mut self, value: u8) -> Self {
        self.value = value;
        self
    }
    pub fn max(mut self, max: u8) -> Self {
        self.max = max.max(1);
        self
    }
    pub fn size(mut self, px: i32) -> Self {
        self.size = px.max(8);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Build the rating row.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let filled = self.value.min(self.max);
        let mut stars: Vec<LayoutNode> = Vec::with_capacity(self.max as usize);
        for i in 0..self.max {
            let color = if i < filled { ColorToken::Warning } else { ColorToken::OnSurfaceVariant };
            stars.push(Icon::new(Symbol::Star).size(self.size).color(color).build(tokens));
        }
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(2),
                padding: EdgeInsets::zero(),
                align: Align::Center,
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
            stars,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::ShapeKind;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn renders_max_stars_filled_up_to_value() {
        let t = BaseTokens;
        match Rating::new().value(3).max(5).id("stars").build(&t) {
            LayoutNode::Stack(stack, _, stars) => {
                assert_eq!(stack.id, Some("stars"));
                assert_eq!(stars.len(), 5);
                let star_color = |n: &LayoutNode| match n {
                    LayoutNode::Stack(_, v, _) => {
                        assert!(matches!(v.shape, ShapeKind::Path(_)), "star is a vector symbol");
                        v.background
                    }
                    _ => panic!(),
                };
                assert_eq!(star_color(&stars[0]), Some(t.color(ColorToken::Warning)), "filled");
                assert_eq!(star_color(&stars[2]), Some(t.color(ColorToken::Warning)), "filled");
                assert_eq!(star_color(&stars[3]), Some(t.color(ColorToken::OnSurfaceVariant)), "muted");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn value_clamps_to_max() {
        let t = BaseTokens;
        match Rating::new().value(9).max(4).build(&t) {
            LayoutNode::Stack(_, _, stars) => assert_eq!(stars.len(), 4),
            _ => panic!(),
        }
    }
}
