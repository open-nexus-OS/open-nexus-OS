// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Chat cell view — renders a `ChatMessage` as a themed bubble
//! `LayoutNode`. The chat-specific data-source cell, owned by the chat app
//! (RFC-0067 P2.4, moved out of `nexus-shell-desktop`). The generic
//! `VirtualList`/`List` paints whatever `LayoutNode` this builds — no chat
//! knowledge in the widget or the compositor.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 test

use crate::model::ChatMessage;
use nexus_layout_types::{
    FlexItem, LayoutNode, Rgba8, TextContent, TextNode, TextStyle, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};
use nexus_virtual_list::ItemView;
use nexus_widget_panel::Panel;

/// A single-line label node in the given color (theme-driven by the caller).
fn label(content: &'static str, color: Rgba8) -> LayoutNode {
    LayoutNode::Text(
        TextNode {
            id: None,
            content: TextContent::new(content),
            style: TextStyle { color, ..TextStyle::default() },
            item: FlexItem::default(),
            max_lines: Some(1),
            min_width: None,
            max_width: None,
        },
        VisualStyle::default(),
    )
}

/// The chat cell: renders a [`ChatMessage`] as a themed bubble. This is the only
/// "chat-specific" view code — a data-source cell, owned by the chat app.
pub struct ChatItemView<'a> {
    pub tokens: &'a dyn Tokens,
}

impl ItemView for ChatItemView<'_> {
    type Item = ChatMessage;

    fn build_item(&self, _index: usize, msg: &ChatMessage) -> LayoutNode {
        let (bubble, text) = if msg.from_me {
            (ColorToken::Accent, ColorToken::OnAccent)
        } else {
            (ColorToken::SurfaceVariant, ColorToken::OnSurface)
        };
        Panel::row()
            .style(
                Style::new()
                    .background_token(self.tokens, bubble)
                    .rounded_token(self.tokens, LengthToken::RadiusMedium),
            )
            .padding(self.tokens.length(LengthToken::SpacingSmall))
            .child(label(msg.text, self.tokens.color(text)))
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    fn stack_bg(node: &LayoutNode) -> Option<Rgba8> {
        match node {
            LayoutNode::Stack(_, v, _) => v.background,
            _ => None,
        }
    }

    #[test]
    fn chat_bubbles_are_themed_by_direction() {
        let t = BaseTokens;
        let view = ChatItemView { tokens: &t };
        let mine = view.build_item(0, &ChatMessage { text: "hi", from_me: true });
        let theirs = view.build_item(1, &ChatMessage { text: "yo", from_me: false });
        assert_ne!(stack_bg(&mine), stack_bg(&theirs), "outgoing vs incoming differ");
        assert_eq!(stack_bg(&mine), Some(t.color(ColorToken::Accent)));
    }
}
