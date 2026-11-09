// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Window chrome primitives (design `WindowButton`/`WindowControls`/
//! `WindowPane`): the title-bar icon button, the minimise·maximise·close
//! cluster, and the inner content card. Pure builders producing
//! `LayoutNode::Stack`s from theme tokens + the [`Icon`] primitive. The window
//! kit's DSL components are the single source of window UI; these builders are
//! kept on the design's values, not grown (TASK-0353).

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, Overflow,
    Spacer, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, RadiusToken, TextStyleToken, Tokens, HAIRLINE};
use nexus_widget_icon::{with_ink, Icon, LucideSymbol};
use nexus_widget_text::Text;

/// Chrome-button treatment (design `WindowButton` active/danger).
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

/// A title-bar / pane-header icon button: a ghost circle (30 px, window controls 36) whose
/// glyph is `labelSecondary` — `label` on the persistent `fill` layer when active, `danger` for
/// close (the glyph only, no background swap).
#[derive(Debug, Clone)]
pub struct WindowButton {
    icon: LayoutNode,
    kind: WindowButtonKind,
    size: i32,
    id: Option<&'static str>,
}

impl WindowButton {
    pub fn new(icon: LayoutNode) -> Self {
        Self { icon, kind: WindowButtonKind::Ghost, size: 30, id: None }
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
        let mut style = Style::new().rounded(FxPx::new(self.size / 2));
        let ink = match self.kind {
            WindowButtonKind::Ghost => ColorToken::LabelSecondary,
            WindowButtonKind::Active => {
                style = style.background(tokens.color(ColorToken::Fill));
                ColorToken::Label
            }
            WindowButtonKind::Danger => ColorToken::Danger,
        };
        let icon = with_ink(self.icon, tokens.color(ink));
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
                text_fit: None,
            },
            style.visual(),
            alloc::vec![icon],
        )
    }
}

/// A window control's diameter (the md control height).
const CONTROL: i32 = 36;

/// The minimise / maximise / close cluster: 36 px circles, 2 px apart; `minus` 14, `square` 13
/// and `x` 14 at stroke 2.5, the close glyph in `danger`.
#[derive(Debug, Clone)]
pub struct WindowControls {
    minimize: Option<&'static str>,
    maximize: Option<&'static str>,
    close: Option<&'static str>,
    gap: i32,
}

impl Default for WindowControls {
    fn default() -> Self {
        Self { minimize: None, maximize: None, close: None, gap: 2 }
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

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let glyph = |symbol: LucideSymbol, px: i32| {
            Icon::lucide(symbol).stroke_tenths(25).size(px).build(tokens)
        };
        let mut row: Vec<LayoutNode> = Vec::new();
        if let Some(id) = self.minimize {
            let icon = glyph(LucideSymbol::Minus, 14);
            row.push(WindowButton::new(icon).size(CONTROL).id(id).build(tokens));
        }
        if let Some(id) = self.maximize {
            let icon = glyph(LucideSymbol::Square, 13);
            row.push(WindowButton::new(icon).size(CONTROL).id(id).build(tokens));
        }
        if let Some(id) = self.close {
            let icon = glyph(LucideSymbol::Xmark, 14);
            let button = WindowButton::new(icon).kind(WindowButtonKind::Danger).size(CONTROL);
            row.push(button.id(id).build(tokens));
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
                text_fit: None,
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
            .background(tokens.color(ColorToken::SurfacePane))
            .rounded(RadiusToken::Md.px())
            .border(HAIRLINE, tokens.color(ColorToken::Separator));

        let mut col: Vec<LayoutNode> = Vec::new();
        if self.title.is_some() || self.header_actions.is_some() {
            let mut header: Vec<LayoutNode> = Vec::new();
            if let Some(title) = self.title {
                header.push(
                    Text::new(title)
                        .text_style(TextStyleToken::Headline)
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
                    text_fit: None,
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
                text_fit: None,
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
                text_fit: None,
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
    fn window_button_is_a_circle_and_active_is_the_fill_layer() {
        let t = BaseTokens;
        let icon = || Icon::lucide(LucideSymbol::SidebarLeft).size(14).build(&t);
        match WindowButton::new(icon()).kind(WindowButtonKind::Active).build(&t) {
            LayoutNode::Stack(s, v, kids) => {
                assert_eq!(s.min_width, Some(FxPx::new(30)));
                assert_eq!(v.corner_radius.top_left, FxPx::new(15), "a circle");
                assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
                match &kids[0] {
                    LayoutNode::Stack(_, gv, _) => {
                        assert_eq!(gv.background, Some(t.color(ColorToken::Label)))
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
        match WindowButton::new(icon()).build(&t) {
            LayoutNode::Stack(_, v, kids) => {
                assert_eq!(v.background, None, "a ghost at rest");
                match &kids[0] {
                    LayoutNode::Stack(_, gv, _) => {
                        assert_eq!(gv.background, Some(t.color(ColorToken::LabelSecondary)))
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }

    #[test]
    fn controls_are_36_px_circles_2_px_apart() {
        let t = BaseTokens;
        match WindowControls::new().minimize("min").maximize("max").close("close").build(&t) {
            LayoutNode::Stack(row, _, btns) => {
                assert_eq!(row.gap, FxPx::new(2));
                for b in &btns {
                    match b {
                        LayoutNode::Stack(s, v, _) => {
                            assert_eq!(s.min_width, Some(FxPx::new(36)));
                            assert_eq!(v.corner_radius.top_left, FxPx::new(18));
                        }
                        _ => panic!(),
                    }
                }
                match &btns[2] {
                    LayoutNode::Stack(_, _, g) => match &g[0] {
                        LayoutNode::Stack(gs, gv, _) => {
                            assert_eq!(gs.min_width, Some(FxPx::new(14)));
                            assert_eq!(gv.background, Some(t.color(ColorToken::Danger)));
                        }
                        _ => panic!(),
                    },
                    _ => panic!(),
                }
                match &btns[1] {
                    LayoutNode::Stack(_, _, g) => match &g[0] {
                        LayoutNode::Stack(gs, _, _) => {
                            assert_eq!(gs.min_width, Some(FxPx::new(13)), "the square glyph")
                        }
                        _ => panic!(),
                    },
                    _ => panic!(),
                }
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
                assert_eq!(v.background, Some(t.color(ColorToken::SurfacePane)));
                assert_eq!(col.len(), 2, "header + body");
            }
            _ => panic!(),
        }
    }
}
