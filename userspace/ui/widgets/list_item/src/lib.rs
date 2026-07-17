// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `ListItem` — the design-system settings/list row (handoff `ListItem`): a
//! leading node, a title with an optional subtitle, and a trailing control. A
//! pure builder producing a `LayoutNode::Stack` row. `destructive` renders the
//! title in the danger color; `show_chevron` is exposed as data (the disclosure
//! indicator is an icon the shell adds). DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, Overflow,
    Spacer, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, Tokens};
use nexus_widget_text::Text;

/// A list / settings row.
#[derive(Debug, Clone, Default)]
pub struct ListItem {
    title: String,
    subtitle: Option<String>,
    leading: Option<LayoutNode>,
    trailing: Option<LayoutNode>,
    show_chevron: bool,
    destructive: bool,
    id: Option<&'static str>,
}

impl ListItem {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), ..Self::default() }
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
    pub fn leading(mut self, leading: LayoutNode) -> Self {
        self.leading = Some(leading);
        self
    }
    /// Trailing control (toggle, value text, badge).
    pub fn trailing(mut self, trailing: LayoutNode) -> Self {
        self.trailing = Some(trailing);
        self
    }
    /// Request a navigation disclosure chevron (rendered by the shell as an icon).
    pub fn show_chevron(mut self, show: bool) -> Self {
        self.show_chevron = show;
        self
    }
    /// Render the title (and, by convention, leading) in the danger color.
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Whether the shell should render the disclosure chevron.
    pub fn wants_chevron(&self) -> bool {
        self.show_chevron
    }

    fn stack(dir: Direction, gap: i32, pad: EdgeInsets, children: Vec<LayoutNode>) -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                id: None,
                direction: dir,
                gap: FxPx::new(gap),
                padding: pad,
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
            children,
        )
    }

    /// Build the list-row node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let title_color = if self.destructive { ColorToken::Danger } else { ColorToken::OnSurface };

        // Title column (title + optional subtitle), left-aligned.
        let mut text_col: Vec<LayoutNode> = Vec::new();
        text_col.push(
            Text::new(self.title).weight(FontWeight::Medium).color(title_color).build(tokens),
        );
        if let Some(subtitle) = self.subtitle {
            text_col.push(Text::caption(subtitle).build(tokens));
        }
        let text_stack = match Self::stack(Direction::Column, 2, EdgeInsets::zero(), text_col) {
            LayoutNode::Stack(mut s, v, c) => {
                s.align = Align::Start;
                LayoutNode::Stack(s, v, c)
            }
            other => other,
        };

        let mut row: Vec<LayoutNode> = Vec::new();
        if let Some(leading) = self.leading {
            row.push(leading);
        }
        row.push(text_stack);
        row.push(LayoutNode::Spacer(Spacer {
            id: None,
            flex_grow: 1,
            min_size: Some(FxPx::new(8)),
            item: FlexItem::default(),
        }));
        if let Some(trailing) = self.trailing {
            row.push(trailing);
        }

        match Self::stack(
            Direction::Row,
            12,
            EdgeInsets::symmetric(FxPx::new(10), FxPx::new(12)),
            row,
        ) {
            LayoutNode::Stack(mut s, v, c) => {
                s.id = self.id;
                LayoutNode::Stack(s, v, c)
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::Spacer as Sp;
    use nexus_theme_tokens::BaseTokens;

    fn ctrl() -> LayoutNode {
        LayoutNode::Spacer(Sp {
            id: None,
            flex_grow: 0,
            min_size: Some(FxPx::new(20)),
            item: FlexItem::default(),
        })
    }

    #[test]
    fn row_has_title_stack_spacer_and_trailing() {
        let t = BaseTokens;
        match ListItem::new("WLAN").subtitle("Verbunden").trailing(ctrl()).id("wifi").build(&t) {
            LayoutNode::Stack(stack, _, children) => {
                assert_eq!(stack.id, Some("wifi"));
                assert_eq!(stack.direction, Direction::Row);
                // title-stack + spacer + trailing (no leading here).
                assert_eq!(children.len(), 3);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn destructive_and_chevron_flag() {
        let t = BaseTokens;
        let item = ListItem::new("Abmelden").destructive(true).show_chevron(true);
        assert!(item.wants_chevron());
        // Title text is danger-colored.
        match item.build(&t) {
            LayoutNode::Stack(_, _, children) => match &children[0] {
                LayoutNode::Stack(_, _, tc) => match &tc[0] {
                    LayoutNode::Text(n, _) => {
                        assert_eq!(n.style.color, t.color(ColorToken::Danger))
                    }
                    _ => panic!(),
                },
                _ => panic!(),
            },
            _ => panic!(),
        }
    }
}
