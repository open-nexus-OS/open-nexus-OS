// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Panel` — a styled flex container widget.
//!
//! A pure builder that turns a [`Style`] (modifiers) + layout props + children
//! into a `LayoutNode::Stack`. Like every widget in the framework it is
//! data-only and side-effect-free (`build() -> LayoutNode`), so the layout
//! engine measures it and the compositor paints the resulting `LayoutBox`es
//! generically — and a future DSL can emit the same builder.
//!
//! Blur (`Style::blur`) is a compositor-level backdrop effect with no
//! `LayoutNode` representation, so it is carried on [`Panel::style`] for the
//! compositor to apply behind the panel's region; `build()` emits the box's
//! `VisualStyle` (background/border/rounded/shadow) + children.

extern crate alloc;

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack,
};
use nexus_style::Style;

/// A styled flex container.
#[derive(Debug, Clone)]
pub struct Panel {
    id: Option<&'static str>,
    style: Style,
    direction: Direction,
    padding: EdgeInsets,
    gap: FxPx,
    align: Align,
    justify: Justify,
    children: Vec<LayoutNode>,
}

impl Default for Panel {
    fn default() -> Self {
        // `Direction`/`Align`/`Justify`/`EdgeInsets` have no `Default`, so spell
        // the neutral container defaults out explicitly.
        Self {
            id: None,
            style: Style::new(),
            direction: Direction::Column,
            padding: EdgeInsets::all(FxPx::ZERO),
            gap: FxPx::ZERO,
            align: Align::Start,
            justify: Justify::Start,
            children: Vec::new(),
        }
    }
}

impl Panel {
    /// A vertical (column) panel.
    pub fn column() -> Self {
        Self { direction: Direction::Column, ..Self::default() }
    }

    /// A horizontal (row) panel.
    pub fn row() -> Self {
        Self { direction: Direction::Row, ..Self::default() }
    }

    /// Debug/marker id for the underlying box.
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Visual modifiers (background/border/rounded/shadow/blur).
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Uniform inner padding.
    pub fn padding(mut self, p: FxPx) -> Self {
        self.padding = EdgeInsets::all(p);
        self
    }

    /// Gap between children along the main axis.
    pub fn gap(mut self, gap: FxPx) -> Self {
        self.gap = gap;
        self
    }

    /// Cross-axis alignment.
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Main-axis distribution.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    /// Append one child.
    pub fn child(mut self, child: LayoutNode) -> Self {
        self.children.push(child);
        self
    }

    /// Replace the children.
    pub fn children(mut self, children: Vec<LayoutNode>) -> Self {
        self.children = children;
        self
    }

    /// The panel's style (so the compositor can read e.g. the backdrop blur,
    /// which has no `LayoutNode` representation).
    pub fn current_style(&self) -> &Style {
        &self.style
    }

    /// Build the layout-node subtree for this panel.
    pub fn build(self) -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: self.direction,
                gap: self.gap,
                padding: self.padding,
                align: self.align,
                justify: self.justify,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            self.style.visual(),
            self.children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{CornerRadius, Rgba8, Spacer};

    fn spacer() -> LayoutNode {
        LayoutNode::Spacer(Spacer { id: None, flex_grow: 1, min_size: None, item: FlexItem::default() })
    }

    #[test]
    fn panel_builds_styled_stack_with_children() {
        let node = Panel::column()
            .id("test_panel")
            .style(
                Style::new()
                    .background(Rgba8::new(20, 24, 32, 255))
                    .rounded(FxPx::new(16))
                    .blur(20, 140),
            )
            .padding(FxPx::new(12))
            .gap(FxPx::new(8))
            .child(spacer())
            .child(spacer())
            .build();

        match node {
            LayoutNode::Stack(stack, visual, children) => {
                assert_eq!(stack.id, Some("test_panel"));
                assert_eq!(stack.direction, Direction::Column);
                assert_eq!(stack.gap, FxPx::new(8));
                assert_eq!(visual.background, Some(Rgba8::new(20, 24, 32, 255)));
                assert_eq!(visual.corner_radius, CornerRadius::uniform(FxPx::new(16)));
                assert_eq!(children.len(), 2);
            }
            _ => panic!("Panel must build a Stack"),
        }
    }

    #[test]
    fn panel_carries_blur_for_the_compositor() {
        // Blur has no LayoutNode representation; the compositor reads it off the style.
        let p = Panel::column().style(Style::new().blur(20, 140));
        assert!(p.current_style().backdrop_blur().is_some());
    }
}
