// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! The **desktop shell** — one pluggable shell UI, assembled purely from the
//! widget library + theme tokens. The `systemui` shell-host service resolves it
//! by `manifests/shells/desktop/shell.toml` for the `desktop` profile and hands
//! its scene to windowd, which composites it on the **virgl/GPU** path.
//!
//! This crate is *pure*: `build_desktop_scene(tokens) -> LayoutNode` and a chat
//! `ItemView`. No app state, no rendering, no compositor knowledge — the
//! framework/app split, and a 1:1 target for the future DSL. A different shell
//! (tablet/kiosk) is a different crate selected by a different manifest; the same
//! widgets + theme, different composition + affordances.
//!
//! The "chat" is just a `VirtualList<ChatMessageProvider>` + [`ChatItemView`]
//! placed in the window's list viewport — content of the shell, not windowd.

extern crate alloc;

use alloc::{vec, vec::Vec};
use nexus_layout_types::{
    FlexItem, FxPx, LayoutNode, Rgba8, TextContent, TextNode, TextStyle, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};
use nexus_virtual_list::{ChatMessage, ItemView};
use nexus_widget_button::Button;
use nexus_widget_panel::Panel;
use nexus_widget_text_field::TextField;
use nexus_widget_window::Window;

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

/// Build the desktop shell's scene: a top bar (menu + search + chat button) over
/// a chat window (title bar + close + a list viewport). Every color/radius/space
/// comes from `tokens` — rebrand by swapping the theme, switch shells by manifest.
pub fn build_desktop_scene(tokens: &dyn Tokens) -> LayoutNode {
    let on_surface = tokens.color(ColorToken::OnSurface);
    let sp_sm = tokens.length(LengthToken::SpacingSmall);
    let sp_md = tokens.length(LengthToken::SpacingMedium);

    let topbar = Panel::row()
        .id("topbar")
        .style(Style::new().background_token(tokens, ColorToken::SurfaceVariant))
        .padding(sp_sm)
        .gap(sp_sm)
        .child(
            Button::new()
                .id("menu_btn")
                .style(
                    Style::new()
                        .background_token(tokens, ColorToken::Surface)
                        .rounded_token(tokens, LengthToken::RadiusSmall),
                )
                .padding(sp_sm)
                .content(label("menu", on_surface))
                .build(),
        )
        .child(
            TextField::new()
                .id("search")
                .placeholder("Search…")
                .style(
                    Style::new()
                        .background_token(tokens, ColorToken::Surface)
                        .rounded_token(tokens, LengthToken::RadiusSmall),
                )
                .build(),
        )
        .child(
            Button::new()
                .id("chat_btn")
                .style(
                    Style::new()
                        .background_token(tokens, ColorToken::Accent)
                        .rounded_token(tokens, LengthToken::RadiusSmall),
                )
                .padding(sp_sm)
                .content(label("chat", tokens.color(ColorToken::OnAccent)))
                .build(),
        )
        .build();

    // The list viewport: the compositor fills it with the chat VirtualList's
    // visible boxes (windowed/lazy). The shell only declares where it lives.
    let viewport = Panel::column().id("chat_viewport").build();

    let chat_window = Window::new()
        .id("chat_window")
        .titlebar_id("chat_titlebar")
        .style(
            Style::new()
                .background_token(tokens, ColorToken::Surface)
                .rounded_token(tokens, LengthToken::RadiusLarge)
                .blur(20, 140),
        )
        .title(label("Chat", on_surface))
        .close_button(
            "chat_close",
            Style::new().rounded_token(tokens, LengthToken::RadiusSmall),
            label("x", on_surface),
        )
        .body(vec![viewport])
        .build();

    Panel::column()
        .id("desktop_root")
        .style(Style::new().background_token(tokens, ColorToken::Background))
        .padding(sp_md)
        .gap(sp_md)
        .children(vec![topbar, chat_window])
        .build()
}

/// The chat cell: renders a [`ChatMessage`] as a themed bubble. This is the only
/// "chat-specific" code — a data-source cell, living in the shell, not windowd.
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
    fn desktop_scene_is_topbar_over_chat_window() {
        let scene = build_desktop_scene(&BaseTokens);
        let LayoutNode::Stack(root, visual, children) = scene else { panic!("root is a Stack") };
        assert_eq!(root.id, Some("desktop_root"));
        assert!(visual.background.is_some(), "root has a themed background");
        assert_eq!(children.len(), 2, "topbar + chat window");
        // Chat window child carries the close button (drill into title bar).
        let LayoutNode::Stack(win, _, win_children) = &children[1] else { panic!("window is a Stack") };
        assert_eq!(win.id, Some("chat_window"));
        let LayoutNode::Stack(tb, _, tb_kids) = &win_children[0] else { panic!("titlebar") };
        assert_eq!(tb.id, Some("chat_titlebar"));
        let LayoutNode::Stack(close, _, _) = tb_kids.last().unwrap() else { panic!("close") };
        assert_eq!(close.id, Some("chat_close"));
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
