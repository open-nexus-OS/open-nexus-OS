// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `GlassCard` — the design-system frosted-glass container (handoff `GlassCard`):
//! the foundational liquid-glass surface every panel/card/row is built on. A
//! pure builder that resolves a [`MaterialToken`] level into a `Style` (fill
//! tint + top-shine border + backdrop blur) and delegates the box + children to
//! the [`Panel`] container primitive. DSL-emittable.

extern crate alloc;

use alloc::vec::Vec;
use nexus_layout_types::{FxPx, LayoutNode};
use nexus_style::Style;
use nexus_theme_tokens::{LengthToken, MaterialToken, Tokens};
use nexus_widget_panel::Panel;

/// Surface level (handoff `GlassCard.variant`). `dock`/`inner` reuse the
/// panel/card materials; `window`/`overlay` expose the denser materials for the
/// window and overlay components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardLevel {
    #[default]
    Panel,
    Dock,
    Card,
    Inner,
    Subtle,
    Window,
    Overlay,
}

impl CardLevel {
    /// The glass material backing this level.
    pub fn material(self) -> MaterialToken {
        match self {
            CardLevel::Panel | CardLevel::Dock => MaterialToken::Panel,
            CardLevel::Card | CardLevel::Inner => MaterialToken::Card,
            CardLevel::Subtle => MaterialToken::Subtle,
            CardLevel::Window => MaterialToken::Window,
            CardLevel::Overlay => MaterialToken::Overlay,
        }
    }

    /// Corner radius token for this level.
    pub fn radius(self) -> LengthToken {
        match self {
            CardLevel::Subtle => LengthToken::RadiusSmall,
            CardLevel::Card | CardLevel::Inner => LengthToken::RadiusMedium,
            _ => LengthToken::RadiusLarge,
        }
    }
}

/// A frosted-glass container.
#[derive(Debug, Clone, Default)]
pub struct GlassCard {
    level: CardLevel,
    row: bool,
    id: Option<&'static str>,
    padding: FxPx,
    children: Vec<LayoutNode>,
}

impl GlassCard {
    /// A vertical (column) card.
    pub fn new() -> Self {
        Self::default()
    }

    /// Lay children out horizontally instead of vertically.
    pub fn row(mut self) -> Self {
        self.row = true;
        self
    }

    pub fn level(mut self, level: CardLevel) -> Self {
        self.level = level;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    pub fn padding(mut self, p: FxPx) -> Self {
        self.padding = p;
        self
    }

    pub fn child(mut self, child: LayoutNode) -> Self {
        self.children.push(child);
        self
    }

    pub fn children(mut self, children: Vec<LayoutNode>) -> Self {
        self.children = children;
        self
    }

    /// The resolved glass [`Style`] (fill tint + shine border + backdrop blur).
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let g = tokens.glass(self.level.material());
        let mut s = Style::new()
            .background(g.tint)
            .rounded(tokens.length(self.level.radius()))
            .blur(g.blur_radius, g.saturation);
        if let Some(border) = g.border {
            s = s.border(tokens.length(LengthToken::BorderThin), border);
        }
        s
    }

    /// Build the glass container node (delegates the box to [`Panel`]).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let style = self.style(tokens);
        let mut panel = if self.row { Panel::row() } else { Panel::column() };
        panel = panel.style(style).padding(self.padding).children(self.children);
        if let Some(id) = self.id {
            panel = panel.id(id);
        }
        panel.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::{BaseTokens, DarkTokens};

    #[test]
    fn levels_map_to_materials() {
        assert_eq!(CardLevel::Panel.material(), MaterialToken::Panel);
        assert_eq!(CardLevel::Dock.material(), MaterialToken::Panel);
        assert_eq!(CardLevel::Inner.material(), MaterialToken::Card);
        assert_eq!(CardLevel::Subtle.material(), MaterialToken::Subtle);
    }

    #[test]
    fn style_is_frosted_from_the_material() {
        let t = DarkTokens;
        let s = GlassCard::new().level(CardLevel::Panel).style(&t);
        let g = t.glass(MaterialToken::Panel);
        assert_eq!(s.visual().background, Some(g.tint));
        assert_eq!(s.backdrop_blur().map(|b| b.radius), Some(g.blur_radius));
        assert!(s.visual().border.top.is_some(), "shine/border from material");
    }

    #[test]
    fn subtle_is_less_blurred_than_panel() {
        let t = BaseTokens;
        let panel = GlassCard::new().level(CardLevel::Panel).style(&t);
        let subtle = GlassCard::new().level(CardLevel::Subtle).style(&t);
        assert!(
            subtle.backdrop_blur().unwrap().radius < panel.backdrop_blur().unwrap().radius,
            "subtle rows blur less than panels"
        );
    }

    #[test]
    fn builds_a_panel_with_id_and_children() {
        let t = BaseTokens;
        let node = GlassCard::new()
            .id("control-center")
            .level(CardLevel::Card)
            .child(LayoutNode::Spacer(nexus_layout_types::Spacer {
                id: None,
                flex_grow: 0,
                min_size: Some(FxPx::new(10)),
                item: nexus_layout_types::FlexItem::default(),
            }))
            .build(&t);
        match node {
            LayoutNode::Stack(stack, visual, children) => {
                assert_eq!(stack.id, Some("control-center"));
                assert!(visual.background.is_some());
                assert_eq!(children.len(), 1);
            }
            _ => panic!("GlassCard must build a Stack (via Panel)"),
        }
    }
}
