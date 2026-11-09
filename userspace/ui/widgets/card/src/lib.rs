// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `GlassCard` — the design-system glass container (the design artifacts' `GlassCard`): the
//! surface every panel, dock pill and nested card is built on. Only `panel` (regular blur) and
//! `dock` (thick blur) blur — the one blur of their stack, and `.material(false)` drops it when
//! the panel itself sits on a blur surface; `card`, `inner` and `subtle` are painted layers on
//! a blur surface and never blur; `overlay` is the opaque floating surface.
//!
//! | level   | fill           | edge        | shadow                   | radius |
//! |---------|----------------|-------------|--------------------------|--------|
//! | panel   | glassPanel     | glassEdge   | shadow4 + edgeHighlight  | xl 24  |
//! | dock    | glassPanel     | glassEdge   | shadow4 + edgeHighlight  | xl 24  |
//! | card    | glassCard      | glassEdge   | shadow2                  | lg 16  |
//! | inner   | glassCard      | glassEdge   | —                        | lg 16  |
//! | subtle  | glassCard      | —           | —                        | lg 16  |
//! | overlay | surfaceOverlay | glassEdge   | shadow3 + edgeHighlight  | xl 24  |
//!
//! Every level clips its content. A nested container takes the computed concentric radius
//! (`.concentric(parent_radius, padding)`). A pure builder resolving the level into a `Style`
//! (through `Style::material`, the one recipe) and delegating the box to the [`Panel`]
//! container primitive. DSL-emittable.

extern crate alloc;

use alloc::vec::Vec;
use nexus_layout_types::{FxPx, LayoutNode, Material, Overflow};
use nexus_style::Style;
use nexus_theme_tokens::{concentric, ColorToken, RadiusToken, ShadowToken, Tokens};
use nexus_widget_panel::Panel;

/// Surface level (the design's `GlassCard.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardLevel {
    /// A panel on the wallpaper (Control Center, notifications): regular blur.
    #[default]
    Panel,
    /// The dock / taskbar pill: thick blur.
    Dock,
    /// A card on a blur surface: `shadow2`, no blur.
    Card,
    /// A dense inner grouping: no shadow, no blur.
    Inner,
    /// The lightest layer (rows): no edge, no shadow, no blur.
    Subtle,
    /// The opaque floating panel (sheets, dropdown panels): `surfaceOverlay`, no blur.
    Overlay,
}

impl CardLevel {
    /// The material backing this level.
    pub fn material(self) -> Material {
        match self {
            CardLevel::Panel => Material::Panel,
            CardLevel::Dock => Material::Dock,
            CardLevel::Card | CardLevel::Inner | CardLevel::Subtle => Material::Card,
            CardLevel::Overlay => Material::Overlay,
        }
    }

    /// Corner radius token for this level: xl for the surfaces, lg for the layers on them.
    pub fn radius(self) -> RadiusToken {
        match self {
            CardLevel::Panel | CardLevel::Dock | CardLevel::Overlay => RadiusToken::Xl,
            CardLevel::Card | CardLevel::Inner | CardLevel::Subtle => RadiusToken::Lg,
        }
    }
}

/// A glass container.
#[derive(Debug, Clone)]
pub struct GlassCard {
    level: CardLevel,
    material: bool,
    radius: Option<FxPx>,
    row: bool,
    id: Option<&'static str>,
    padding: FxPx,
    children: Vec<LayoutNode>,
}

impl Default for GlassCard {
    fn default() -> Self {
        Self {
            level: CardLevel::default(),
            material: true,
            radius: None,
            row: false,
            id: None,
            padding: FxPx::ZERO,
            children: Vec::new(),
        }
    }
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

    /// Panel and dock only: `false` keeps the fill but drops the backdrop blur (the panel sits
    /// on another blur surface — never blur over blur). No effect on the other levels.
    pub fn material(mut self, material: bool) -> Self {
        self.material = material;
        self
    }

    /// An explicit corner radius (default: the level's).
    pub fn radius(mut self, radius: FxPx) -> Self {
        self.radius = Some(radius);
        self
    }

    /// The radius of a container nested in one of radius `parent` with `padding`.
    pub fn concentric(self, parent: FxPx, padding: FxPx) -> Self {
        self.radius(concentric(parent, padding))
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

    /// The resolved surface [`Style`] (fill, edge, shadow, lit edge, radius, backdrop blur).
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let radius = self.radius.unwrap_or_else(|| self.level.radius().px());
        let level = self.level;
        if !self.material && level.material().blurs() {
            // The same surface without its blur: a plain painted box (no material, so the
            // compositor makes no region of it).
            return Style::new()
                .background_token(tokens, ColorToken::GlassPanel)
                .border_token(tokens, ColorToken::GlassEdge)
                .shadow_token(tokens, ShadowToken::Shadow4)
                .shadow_token(tokens, ShadowToken::EdgeHighlight)
                .rounded(radius);
        }
        // `Style::material` is the ONE definition of a material's recipe.
        let s = Style::new().material(level.material(), tokens).rounded(radius);
        match level {
            // The dock is the panel's elevation on the thick blur.
            CardLevel::Dock => s.shadow_token(tokens, ShadowToken::Shadow4),
            CardLevel::Card => s.shadow_token(tokens, ShadowToken::Shadow2),
            CardLevel::Subtle => s.no_border(),
            CardLevel::Panel | CardLevel::Inner | CardLevel::Overlay => s,
        }
    }

    /// Build the glass container node (delegates the box to [`Panel`]; it clips its content).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let style = self.style(tokens);
        let mut panel = if self.row { Panel::row() } else { Panel::column() };
        panel = panel.style(style).padding(self.padding).children(self.children);
        if let Some(id) = self.id {
            panel = panel.id(id);
        }
        let mut node = panel.build();
        if let LayoutNode::Stack(stack, _, _) = &mut node {
            stack.overflow = Overflow::Hidden;
        }
        node
    }
}

#[cfg(test)]
mod tests;
