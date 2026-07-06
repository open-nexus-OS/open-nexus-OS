// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Window` — a chrome container: title bar (title + close button) over a body.
//!
//! Composed from [`Panel`](nexus_widget_panel::Panel) (column chrome + row title
//! bar) and [`Button`](nexus_widget_button::Button) (the close control), so it
//! reuses the modifier/layout stack rather than re-implementing chrome. A pure
//! builder → `LayoutNode`. Window *state* (bounds, drag, z-order) is not here —
//! that is a window-manager concern; this is only the visual structure, and the
//! close control is exposed by `id` for the compositor to hit-test (app maps
//! id → close action). Drag is driven by hit-testing the title-bar `id`.

extern crate alloc;

/// Pure window-frame geometry (hit-testing / drag clamp / damage) — the
/// host-tested SSOT shared by every window instance (RFC-0067 P3).
pub mod frame;
pub use frame::{Frame, ResizeEdge, TitleButton, WindowPress};

pub mod chrome;
pub use chrome::{WindowButton, WindowButtonKind, WindowControls, WindowPane};

use alloc::vec::Vec;
use nexus_layout_types::{Align, FlexItem, FxPx, LayoutNode, Spacer};
use nexus_style::Style;
use nexus_widget_button::Button;
use nexus_widget_panel::Panel;

/// A window chrome: title bar (title + close) + body content.
#[derive(Debug, Clone)]
pub struct Window {
    id: Option<&'static str>,
    titlebar_id: Option<&'static str>,
    style: Style,
    titlebar_padding: FxPx,
    title: Option<LayoutNode>,
    close: Option<CloseButton>,
    body: Vec<LayoutNode>,
}

#[derive(Debug, Clone)]
struct CloseButton {
    id: &'static str,
    style: Style,
    content: Option<LayoutNode>,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            id: None,
            titlebar_id: None,
            style: Style::new(),
            titlebar_padding: FxPx::new(8),
            title: None,
            close: None,
            body: Vec::new(),
        }
    }
}

impl Window {
    pub fn new() -> Self {
        Self::default()
    }

    /// Window id (whole-window region, e.g. for focus/z-order).
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Title-bar id — the compositor hit-tests this region to start a drag.
    pub fn titlebar_id(mut self, id: &'static str) -> Self {
        self.titlebar_id = Some(id);
        self
    }

    /// Window chrome style (background/border/rounded/shadow/blur).
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Title content node (caller-provided text/icon).
    pub fn title(mut self, title: LayoutNode) -> Self {
        self.title = Some(title);
        self
    }

    /// Add a close button: interaction `id` (app maps to close), its style, and
    /// the glyph node (e.g. an "X").
    pub fn close_button(mut self, id: &'static str, style: Style, content: LayoutNode) -> Self {
        self.close = Some(CloseButton { id, style, content: Some(content) });
        self
    }

    /// Body content (below the title bar).
    pub fn body(mut self, body: Vec<LayoutNode>) -> Self {
        self.body = body;
        self
    }

    /// Build the window's layout-node subtree.
    pub fn build(self) -> LayoutNode {
        // Title bar: [title?] [flexible spacer] [close?]
        let mut titlebar = Panel::row().align(Align::Center).padding(self.titlebar_padding).gap(FxPx::new(8));
        if let Some(id) = self.titlebar_id {
            titlebar = titlebar.id(id);
        }
        if let Some(title) = self.title {
            titlebar = titlebar.child(title);
        }
        // Spacer pushes the close button to the trailing edge.
        titlebar = titlebar.child(LayoutNode::Spacer(Spacer {
            id: None,
            flex_grow: 1,
            min_size: None,
            item: FlexItem::default(),
        }));
        if let Some(close) = self.close {
            let mut btn = Button::new().id(close.id).style(close.style);
            if let Some(content) = close.content {
                btn = btn.content(content);
            }
            titlebar = titlebar.child(btn.build());
        }

        // Window = column: title bar + body.
        let mut children = Vec::with_capacity(1 + self.body.len());
        children.push(titlebar.build());
        children.extend(self.body);

        let mut panel = Panel::column().style(self.style).children(children);
        if let Some(id) = self.id {
            panel = panel.id(id);
        }
        panel.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{Direction, Rgba8};

    fn glyph(id: &'static str) -> LayoutNode {
        LayoutNode::Spacer(Spacer {
            id: Some(id),
            flex_grow: 0,
            min_size: Some(FxPx::new(14)),
            item: FlexItem::default(),
        })
    }

    #[test]
    fn window_has_titlebar_with_close_then_body() {
        let node = Window::new()
            .id("chat")
            .titlebar_id("chat_titlebar")
            .style(Style::new().background(Rgba8::new(20, 24, 32, 230)).rounded(FxPx::new(16)))
            .title(glyph("chat_title"))
            .close_button("chat_close", Style::new().rounded(FxPx::new(8)), glyph("x"))
            .body(alloc::vec![glyph("body_a"), glyph("body_b")])
            .build();

        // Outer = column window with id "chat": [titlebar, body_a, body_b]
        let LayoutNode::Stack(stack, visual, children) = node else { panic!("window is a Stack") };
        assert_eq!(stack.id, Some("chat"));
        assert_eq!(stack.direction, Direction::Column);
        assert!(visual.background.is_some());
        assert_eq!(children.len(), 3, "titlebar + 2 body nodes");

        // First child = the title bar row, ending in the close button.
        let LayoutNode::Stack(tb, _, tb_children) = &children[0] else { panic!("titlebar is a Stack") };
        assert_eq!(tb.id, Some("chat_titlebar"));
        assert_eq!(tb.direction, Direction::Row);
        let last = tb_children.last().expect("close button present");
        let LayoutNode::Stack(close, _, _) = last else { panic!("close is a Button(Stack)") };
        assert_eq!(close.id, Some("chat_close"));
    }
}
