// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Chip` — the design-system chip (handoff `Chip`): a compact glass token for
//! filters/tags/recipients, larger and more tactile than a `Badge`; selectable
//! (accent when selected) and removable (a trailing remove node). A pure builder
//! producing a rounded-full `LayoutNode::Stack`. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack,
};
use nexus_style::{InteractionState, Style};
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};
use nexus_widget_text::Text;

const PILL_RADIUS: i32 = 999;

/// A selectable/removable chip.
#[derive(Debug, Clone, Default)]
pub struct Chip {
    label: String,
    selected: bool,
    leading: Option<LayoutNode>,
    remove: Option<LayoutNode>,
    state: InteractionState,
    id: Option<&'static str>,
}

impl Chip {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), ..Self::default() }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    /// Leading node (an icon).
    pub fn leading(mut self, leading: LayoutNode) -> Self {
        self.leading = Some(leading);
        self
    }
    /// Trailing remove affordance (a "×" node with its own id).
    pub fn removable(mut self, remove: LayoutNode) -> Self {
        self.remove = Some(remove);
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

    /// The label ink for the current selected state.
    pub fn foreground(&self) -> ColorToken {
        if self.selected {
            ColorToken::OnAccent
        } else {
            ColorToken::OnSurface
        }
    }

    /// Build the chip node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let fg = self.foreground();
        let bg = if self.selected { ColorToken::Accent } else { ColorToken::SurfaceVariant };
        let mut style = Style::new().background(tokens.color(bg)).rounded(FxPx::new(PILL_RADIUS));
        if !self.selected {
            style = style.border_token(tokens, LengthToken::BorderThin, ColorToken::Border);
        }
        if self.state.is_disabled() {
            style = style.opacity(self.state.opacity());
        }

        let mut children: Vec<LayoutNode> = Vec::new();
        if let Some(leading) = self.leading {
            children.push(leading);
        }
        children.push(Text::new(self.label).color(fg).build(tokens));
        if let Some(remove) = self.remove {
            children.push(remove);
        }

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(6),
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
            style.visual(),
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn selected_uses_accent_fill_and_on_accent_ink() {
        let t = BaseTokens;
        let chip = Chip::new("Design").selected(true);
        assert_eq!(chip.foreground(), ColorToken::OnAccent);
        match chip.build(&t) {
            LayoutNode::Stack(_, v, children) => {
                assert_eq!(v.background, Some(t.color(ColorToken::Accent)));
                assert!(v.border.top.is_none(), "selected chips drop the outline");
                assert_eq!(children.len(), 1, "just the label");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn unselected_is_outlined_and_removable_adds_a_child() {
        let t = BaseTokens;
        use nexus_layout_types::{FlexItem, Spacer};
        let x = LayoutNode::Spacer(Spacer {
            id: None,
            flex_grow: 0,
            min_size: Some(FxPx::new(12)),
            item: FlexItem::default(),
        });
        match Chip::new("anna@firma.de").removable(x).build(&t) {
            LayoutNode::Stack(_, v, children) => {
                assert_eq!(v.background, Some(t.color(ColorToken::SurfaceVariant)));
                assert!(v.border.top.is_some());
                assert_eq!(children.len(), 2, "label + remove");
            }
            _ => panic!(),
        }
    }
}
