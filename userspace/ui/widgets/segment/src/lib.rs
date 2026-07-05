// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Segment` — the design-system segmented control (handoff `Segment`): a glass
//! pill with a sliding thumb behind the active option. A pure builder producing
//! a `LayoutNode::Stack` row of option slots; the active slot carries the thumb
//! fill. Options are caller-provided nodes (they own their labels/ids), so the
//! app maps the tapped option → value. DSL-emittable.

extern crate alloc;

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};

/// Size preset (handoff `SegmentProps.size`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl SegmentSize {
    fn slot_padding(self) -> EdgeInsets {
        let (v, h) = match self {
            SegmentSize::Sm => (3, 8),
            SegmentSize::Md => (5, 12),
            SegmentSize::Lg => (7, 16),
        };
        EdgeInsets::symmetric(FxPx::new(v), FxPx::new(h))
    }
}

/// A segmented control.
#[derive(Debug, Clone, Default)]
pub struct Segment {
    active: usize,
    size: SegmentSize,
    id: Option<&'static str>,
    options: Vec<LayoutNode>,
}

impl Segment {
    pub fn new() -> Self {
        Self::default()
    }

    /// Index of the active option (its slot gets the sliding thumb).
    pub fn active(mut self, active: usize) -> Self {
        self.active = active;
        self
    }

    pub fn size(mut self, size: SegmentSize) -> Self {
        self.size = size;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The option nodes (caller-built labels; each may carry its own id).
    pub fn options(mut self, options: Vec<LayoutNode>) -> Self {
        self.options = options;
        self
    }

    fn slot(&self, tokens: &dyn Tokens, index: usize, option: LayoutNode) -> LayoutNode {
        let mut style = Style::new().rounded(tokens.length(LengthToken::RadiusSmall));
        if index == self.active {
            // The thumb: a lighter surface pill behind the active option.
            style = style.background(tokens.color(ColorToken::Surface));
        }
        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: self.size.slot_padding(),
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            style.visual(),
            alloc::vec![option],
        )
    }

    /// Build the segmented control node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        // Track pill.
        let track_style = Style::new()
            .background(tokens.color(ColorToken::SurfaceVariant))
            .rounded(tokens.length(LengthToken::RadiusMedium));
        let mut slots = Vec::with_capacity(self.options.len());
        for (i, option) in self.options.clone().into_iter().enumerate() {
            slots.push(self.slot(tokens, i, option));
        }
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(2),
                padding: EdgeInsets::all(FxPx::new(2)),
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
            track_style.visual(),
            slots,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{FlexItem, Spacer};
    use nexus_theme_tokens::BaseTokens;

    fn opt() -> LayoutNode {
        LayoutNode::Spacer(Spacer { id: None, flex_grow: 0, min_size: Some(FxPx::new(24)), item: FlexItem::default() })
    }

    #[test]
    fn active_slot_gets_the_thumb_others_transparent() {
        let t = BaseTokens;
        let seg = Segment::new().active(1).options(alloc::vec![opt(), opt(), opt()]).build(&t);
        match seg {
            LayoutNode::Stack(_, visual, slots) => {
                assert_eq!(visual.background, Some(t.color(ColorToken::SurfaceVariant)), "track");
                assert_eq!(slots.len(), 3);
                let slot_bg = |n: &LayoutNode| match n {
                    LayoutNode::Stack(_, v, _) => v.background,
                    _ => None,
                };
                assert_eq!(slot_bg(&slots[1]), Some(t.color(ColorToken::Surface)), "active thumb");
                assert_eq!(slot_bg(&slots[0]), None, "inactive transparent");
                assert_eq!(slot_bg(&slots[2]), None);
            }
            _ => panic!("Segment must build a Stack"),
        }
    }

    #[test]
    fn keeps_id_and_all_options() {
        let t = BaseTokens;
        match Segment::new().id("view").options(alloc::vec![opt(), opt()]).build(&t) {
            LayoutNode::Stack(stack, _, slots) => {
                assert_eq!(stack.id, Some("view"));
                assert_eq!(slots.len(), 2);
            }
            _ => panic!(),
        }
    }
}
