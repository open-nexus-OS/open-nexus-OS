// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Window chrome primitives (handoff `WindowButton`/`WindowControls`/
//! `WindowPane`): the title-bar icon button, the minimise·maximise·close
//! cluster, and the inner content card. Pure builders producing
//! `LayoutNode::Stack`s from theme tokens + the [`Icon`] primitive. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, Overflow,
    Spacer, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens, TypographyToken};
use nexus_widget_icon::{Icon, Symbol};
use nexus_widget_text::Text;

/// Chrome-button treatment (handoff `WindowButton` active/danger).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowButtonKind {
    /// Ghost (transparent at rest).
    #[default]
    Ghost,
    /// Pinned/on (a toggled panel button).
    Active,
    /// Destructive (close).
    Danger,
}

/// A title-bar / pane-header icon button.
#[derive(Debug, Clone)]
pub struct WindowButton {
    icon: LayoutNode,
    kind: WindowButtonKind,
    size: i32,
    radius: i32,
    id: Option<&'static str>,
}

impl WindowButton {
    pub fn new(icon: LayoutNode) -> Self {
        Self { icon, kind: WindowButtonKind::Ghost, size: 30, radius: 8, id: None }
    }
    pub fn kind(mut self, kind: WindowButtonKind) -> Self {
        self.kind = kind;
        self
    }
    pub fn size(mut self, size: i32) -> Self {
        self.size = size.max(16);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let mut style = Style::new().rounded(FxPx::new(self.radius));
        if matches!(self.kind, WindowButtonKind::Active) {
            style = style.background(tokens.color(ColorToken::SurfaceVariant));
        }
        let d = Some(FxPx::new(self.size));
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem::default(),
            },
            style.visual(),
            alloc::vec![self.icon],
        )
    }
}

/// The minimise / maximise / close cluster.
#[derive(Debug, Clone)]
pub struct WindowControls {
    minimize: Option<&'static str>,
    maximize: Option<&'static str>,
    close: Option<&'static str>,
    gap: i32,
}

impl Default for WindowControls {
    fn default() -> Self {
        Self { minimize: None, maximize: None, close: None, gap: 4 }
    }
}

impl WindowControls {
    pub fn new() -> Self {
        Self::default()
    }

    /// Gap between the buttons (px). A compositor rendering this cluster into
    /// a WM title bar aligns the visuals with its `Frame` hit zones by picking
    /// the gap so each button centers in its zone (e.g. 40 px zones + 30 px
    /// buttons ⇒ gap 10).
    pub fn gap(mut self, gap: i32) -> Self {
        self.gap = gap.max(0);
        self
    }
    pub fn minimize(mut self, id: &'static str) -> Self {
        self.minimize = Some(id);
        self
    }
    pub fn maximize(mut self, id: &'static str) -> Self {
        self.maximize = Some(id);
        self
    }
    pub fn close(mut self, id: &'static str) -> Self {
        self.close = Some(id);
        self
    }

    /// A small square outline (the maximise glyph — no Square symbol needed).
    fn square(tokens: &dyn Tokens) -> LayoutNode {
        let visual = Style::new()
            .rounded(FxPx::new(2))
            .border(FxPx::new(2), tokens.color(ColorToken::OnSurfaceVariant))
            .visual();
        let d = Some(FxPx::new(12));
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
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem::default(),
            },
            visual,
            alloc::vec![],
        )
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let mut row: Vec<LayoutNode> = Vec::new();
        if let Some(id) = self.minimize {
            let icon = Icon::new(Symbol::Minus).size(14).color(ColorToken::OnSurfaceVariant).build(tokens);
            row.push(WindowButton::new(icon).id(id).build(tokens));
        }
        if let Some(id) = self.maximize {
            row.push(WindowButton::new(Self::square(tokens)).id(id).build(tokens));
        }
        if let Some(id) = self.close {
            let icon = Icon::new(Symbol::Close).size(14).color(ColorToken::Danger).build(tokens);
            row.push(WindowButton::new(icon).kind(WindowButtonKind::Danger).id(id).build(tokens));
        }
        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::new(self.gap),
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::End,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            row,
        )
    }
}

/// An inner content card with an optional header (title + actions) and body.
#[derive(Debug, Clone, Default)]
pub struct WindowPane {
    title: Option<String>,
    header_actions: Option<LayoutNode>,
    body: Vec<LayoutNode>,
    padded: bool,
    id: Option<&'static str>,
}

impl WindowPane {
    pub fn new() -> Self {
        Self { padded: true, ..Self::default() }
    }
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    pub fn header_actions(mut self, actions: LayoutNode) -> Self {
        self.header_actions = Some(actions);
        self
    }
    pub fn body(mut self, body: Vec<LayoutNode>) -> Self {
        self.body = body;
        self
    }
    pub fn padded(mut self, padded: bool) -> Self {
        self.padded = padded;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let style = Style::new()
            .background(tokens.color(ColorToken::Surface))
            .rounded(tokens.length(LengthToken::RadiusMedium))
            .border(tokens.length(LengthToken::BorderThin), tokens.color(ColorToken::Border));

        let mut col: Vec<LayoutNode> = Vec::new();
        if self.title.is_some() || self.header_actions.is_some() {
            let mut header: Vec<LayoutNode> = Vec::new();
            if let Some(title) = self.title {
                header.push(
                    Text::new(title)
                        .size(TypographyToken::Md)
                        .weight(FontWeight::Semibold)
                        .build(tokens),
                );
            }
            header.push(LayoutNode::Spacer(Spacer {
                id: None,
                flex_grow: 1,
                min_size: Some(FxPx::new(8)),
                item: FlexItem::default(),
            }));
            if let Some(actions) = self.header_actions {
                header.push(actions);
            }
            col.push(LayoutNode::Stack(
                Stack {
                    id: None,
                    direction: Direction::Row,
                    gap: FxPx::new(8),
                    padding: EdgeInsets::symmetric(FxPx::new(8), FxPx::new(12)),
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
                header,
            ));
        }
        // Body.
        col.push(LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Column,
                gap: FxPx::new(4),
                padding: if self.padded {
                    EdgeInsets::all(FxPx::new(12))
                } else {
                    EdgeInsets::zero()
                },
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            self.body,
        ));

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Column,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: Some(FxPx::new(220)),
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            style.visual(),
            col,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn controls_emit_three_id_carrying_buttons() {
        let t = BaseTokens;
        match WindowControls::new().minimize("min").maximize("max").close("close").build(&t) {
            LayoutNode::Stack(_, _, btns) => {
                assert_eq!(btns.len(), 3);
                let id = |n: &LayoutNode| match n {
                    LayoutNode::Stack(s, _, _) => s.id,
                    _ => None,
                };
                assert_eq!(id(&btns[0]), Some("min"));
                assert_eq!(id(&btns[2]), Some("close"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn window_button_active_gets_surface_bg() {
        let t = BaseTokens;
        let icon = Icon::new(Symbol::Minus).build(&t);
        match WindowButton::new(icon).kind(WindowButtonKind::Active).build(&t) {
            LayoutNode::Stack(_, v, _) => {
                assert_eq!(v.background, Some(t.color(ColorToken::SurfaceVariant)))
            }
            _ => panic!(),
        }
    }

    #[test]
    fn pane_has_header_and_body_on_a_surface() {
        let t = BaseTokens;
        match WindowPane::new().title("Inhalt").body(alloc::vec![]).id("pane").build(&t) {
            LayoutNode::Stack(stack, v, col) => {
                assert_eq!(stack.id, Some("pane"));
                assert_eq!(v.background, Some(t.color(ColorToken::Surface)));
                assert_eq!(col.len(), 2, "header + body");
            }
            _ => panic!(),
        }
    }
}
