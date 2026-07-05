// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Stepper` — the design-system −/+ numeric stepper (handoff `Stepper`): a
//! glass pill with a decrement zone, the value, and an increment zone. A pure
//! builder producing a `LayoutNode::Stack` row. The −/+ glyphs and the value are
//! caller-provided nodes (they carry their own interaction ids), so the app maps
//! the tapped end → value change. DSL-emittable.

extern crate alloc;

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack,
};
use nexus_style::{InteractionState, Style};
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};

/// A −/+ numeric stepper.
#[derive(Debug, Clone, Default)]
pub struct Stepper {
    dec: Option<LayoutNode>,
    value: Option<LayoutNode>,
    inc: Option<LayoutNode>,
    state: InteractionState,
    id: Option<&'static str>,
}

impl Stepper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decrement affordance (a "−" glyph node with its own id).
    pub fn dec(mut self, dec: LayoutNode) -> Self {
        self.dec = Some(dec);
        self
    }
    /// The formatted value node.
    pub fn value(mut self, value: LayoutNode) -> Self {
        self.value = Some(value);
        self
    }
    /// Increment affordance (a "+" glyph node with its own id).
    pub fn inc(mut self, inc: LayoutNode) -> Self {
        self.inc = Some(inc);
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

    fn zone(child: LayoutNode) -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::symmetric(FxPx::new(4), FxPx::new(10)),
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
            nexus_layout_types::VisualStyle::default(),
            alloc::vec![child],
        )
    }

    /// Build the stepper pill node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let mut style = Style::new()
            .background(tokens.color(ColorToken::SurfaceVariant))
            .rounded(tokens.length(LengthToken::RadiusMedium))
            .border_token(tokens, LengthToken::BorderThin, ColorToken::Border);
        if self.state.is_disabled() {
            style = style.opacity(self.state.opacity());
        }

        let mut children: Vec<LayoutNode> = Vec::new();
        if let Some(dec) = self.dec {
            children.push(Self::zone(dec));
        }
        if let Some(value) = self.value {
            children.push(value);
        }
        if let Some(inc) = self.inc {
            children.push(Self::zone(inc));
        }

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(4),
                padding: EdgeInsets::all(FxPx::new(2)),
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
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{FlexItem, Spacer};
    use nexus_theme_tokens::BaseTokens;

    fn glyph() -> LayoutNode {
        LayoutNode::Spacer(Spacer { id: None, flex_grow: 0, min_size: Some(FxPx::new(12)), item: FlexItem::default() })
    }

    #[test]
    fn builds_pill_with_dec_value_inc() {
        let t = BaseTokens;
        match Stepper::new().id("qty").dec(glyph()).value(glyph()).inc(glyph()).build(&t) {
            LayoutNode::Stack(stack, visual, children) => {
                assert_eq!(stack.id, Some("qty"));
                assert_eq!(visual.background, Some(t.color(ColorToken::SurfaceVariant)));
                assert_eq!(children.len(), 3);
            }
            _ => panic!("Stepper must build a Stack"),
        }
    }

    #[test]
    fn disabled_dims() {
        let t = BaseTokens;
        match Stepper::new().state(InteractionState::Disabled).value(glyph()).build(&t) {
            LayoutNode::Stack(_, visual, _) => {
                assert_eq!(visual.opacity, Some(InteractionState::Disabled.opacity()))
            }
            _ => panic!(),
        }
    }
}
