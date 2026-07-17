// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! The **desktop shell** — one pluggable shell UI, assembled purely from the
//! widget library + theme tokens. The `systemui` shell-host service resolves it
//! by `manifests/shells/desktop/shell.toml` for the `desktop` profile and hands
//! its scene to windowd, which composites it on the **virgl/GPU** path.
//!
//! This crate is *pure*: `build_desktop_scene(tokens) -> LayoutNode`. No app
//! state, no rendering, no compositor knowledge — the framework/app split, and a
//! 1:1 target for the future DSL. A different shell (tablet/kiosk) is a different
//! crate selected by a different manifest; the same widgets + theme, different
//! composition + affordances.
//!
//! The chat *content* (message model + cell view) lives in `chat-app`
//! (RFC-0067 P2.4); this shell only lays out the window that hosts it.

extern crate alloc;

/// Host-tested adapter: the desktop scene → positioned `LayoutBox`es via
/// `nexus_layout`, for the compositor to rasterize (RFC-0067 P3).
pub mod desktop_scene;

use alloc::{vec, vec::Vec};
use nexus_layout_types::{
    FlexItem, FxPx, LayoutNode, Rgba8, TextContent, TextNode, TextStyle, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};
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

/// One target-test card: a rounded, themed tile with a centered label — built
/// purely from the widget + style + theme-token components.
fn target_card(
    tokens: &dyn Tokens,
    id: &'static str,
    text: &'static str,
    bg: ColorToken,
) -> LayoutNode {
    Panel::column()
        .id(id)
        .style(
            Style::new()
                .background_token(tokens, bg)
                .rounded_token(tokens, LengthToken::RadiusMedium),
        )
        .padding(tokens.length(LengthToken::SpacingMedium))
        .child(label(text, tokens.color(ColorToken::OnSurface)))
        .build()
}

/// The **target-test panel** — the proof UI ("wie im anderen"), rebuilt from the
/// new components: a rounded, themed surface panel with a title + subtitle and a
/// row of themed cards (hover / click / scroll / keyboard). Every color, radius,
/// and spacing comes from `tokens`; no hardcoded values, no windowd-baked code.
/// windowd lays this out (`nexus_layout`) and rasterizes it into an atlas layer;
/// gpud composites it over the wallpaper.
pub fn build_target_panel(tokens: &dyn Tokens) -> LayoutNode {
    let on_surface = tokens.color(ColorToken::OnSurface);
    let muted = tokens.color(ColorToken::OnSurfaceVariant);
    let sp_sm = tokens.length(LengthToken::SpacingSmall);
    let sp_md = tokens.length(LengthToken::SpacingMedium);

    let cards = Panel::row()
        .id("target_cards")
        .gap(sp_sm)
        .children(vec![
            target_card(tokens, "card_hover", "hover", ColorToken::SurfaceVariant),
            target_card(tokens, "card_click", "click", ColorToken::Accent),
            target_card(tokens, "card_scroll", "scroll", ColorToken::SurfaceVariant),
            target_card(tokens, "card_keyboard", "keyboard", ColorToken::SurfaceVariant),
        ])
        .build();

    Panel::column()
        .id("target_panel")
        .style(
            Style::new()
                .background_token(tokens, ColorToken::Surface)
                .rounded_token(tokens, LengthToken::RadiusLarge)
                .border_token(tokens, LengthToken::BorderThin, ColorToken::Border),
        )
        .padding(sp_md)
        .gap(sp_sm)
        .children(vec![
            label("Target Tests", on_surface),
            label("rendered from ui/ components", muted),
            cards,
        ])
        .build()
}

// The chat cell (`ChatItemView`) moved to `chat-app` (RFC-0067 P2.4): the chat
// app owns its message model + cell view; this shell crate stays content-free.

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
        let LayoutNode::Stack(win, _, win_children) = &children[1] else {
            panic!("window is a Stack")
        };
        assert_eq!(win.id, Some("chat_window"));
        let LayoutNode::Stack(tb, _, tb_kids) = &win_children[0] else { panic!("titlebar") };
        assert_eq!(tb.id, Some("chat_titlebar"));
        let LayoutNode::Stack(close, _, _) = tb_kids.last().unwrap() else { panic!("close") };
        assert_eq!(close.id, Some("chat_close"));
    }

    #[test]
    fn target_panel_is_themed_surface_with_card_row() {
        let t = BaseTokens;
        let scene = build_target_panel(&t);
        let LayoutNode::Stack(root, visual, children) = scene else { panic!("root is a Stack") };
        assert_eq!(root.id, Some("target_panel"));
        assert_eq!(visual.background, Some(t.color(ColorToken::Surface)), "themed surface bg");
        assert!(visual.border.top.is_some(), "themed border");
        assert_ne!(visual.corner_radius, Default::default(), "rounded corners");
        // title + subtitle + the card row.
        let LayoutNode::Stack(cards, _, card_kids) = children.last().unwrap() else {
            panic!("card row")
        };
        assert_eq!(cards.id, Some("target_cards"));
        assert_eq!(card_kids.len(), 4, "hover/click/scroll/keyboard cards");
        // The click card is themed differently (Accent) from the others.
        assert_eq!(stack_bg(&card_kids[1]), Some(t.color(ColorToken::Accent)));
        assert_eq!(stack_bg(&card_kids[0]), Some(t.color(ColorToken::SurfaceVariant)));
    }
}
