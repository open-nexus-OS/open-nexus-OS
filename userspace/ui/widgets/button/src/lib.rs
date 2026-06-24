// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Button` — a styled, identified, clickable container.
//!
//! A pure builder producing a `LayoutNode::Stack` that centers a caller-supplied
//! content node (a label/icon `LayoutNode`). The `id` is the **interaction id**:
//! the widget carries no click handler (no app state in widgets) — the compositor
//! hit-tests the button's rendered rect by `id` and the app maps the id to an
//! action. This is the framework/app split (the target-action / data-source
//! model) and keeps the widget DSL-emittable.

extern crate alloc;

use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack,
};
use nexus_style::Style;

/// A styled clickable container centering its content.
#[derive(Debug, Clone)]
pub struct Button {
    id: Option<&'static str>,
    style: Style,
    padding: EdgeInsets,
    content: Option<LayoutNode>,
}

impl Default for Button {
    fn default() -> Self {
        Self { id: None, style: Style::new(), padding: EdgeInsets::all(FxPx::ZERO), content: None }
    }
}

impl Button {
    pub fn new() -> Self {
        Self::default()
    }

    /// Interaction id — the compositor hit-tests the rendered rect by this id;
    /// the app maps id → action.
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

    /// The label/icon node (caller-provided — widgets don't own text/icons).
    pub fn content(mut self, content: LayoutNode) -> Self {
        self.content = Some(content);
        self
    }

    /// The button's interaction id.
    pub fn interaction_id(&self) -> Option<&'static str> {
        self.id
    }

    /// The button's style (so the compositor can read e.g. the backdrop blur).
    pub fn current_style(&self) -> &Style {
        &self.style
    }

    /// Build the layout-node subtree.
    pub fn build(self) -> LayoutNode {
        let children = match self.content {
            Some(c) => alloc::vec![c],
            None => alloc::vec![],
        };
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: self.padding,
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            self.style.visual(),
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{Rgba8, Spacer};

    #[test]
    fn button_centers_content_and_keeps_id() {
        let node = Button::new()
            .id("close")
            .style(Style::new().background(Rgba8::new(40, 40, 40, 200)).rounded(FxPx::new(8)))
            .padding(FxPx::new(6))
            .content(LayoutNode::Spacer(Spacer {
                id: Some("x_icon"),
                flex_grow: 0,
                min_size: Some(FxPx::new(16)),
                item: FlexItem::default(),
            }))
            .build();
        match node {
            LayoutNode::Stack(stack, visual, children) => {
                assert_eq!(stack.id, Some("close"));
                assert_eq!(stack.align, Align::Center);
                assert_eq!(stack.justify, Justify::Center);
                assert!(visual.background.is_some());
                assert_eq!(children.len(), 1);
            }
            _ => panic!("Button must build a Stack"),
        }
    }

    #[test]
    fn button_exposes_interaction_id() {
        let b = Button::new().id("sidebar_toggle");
        assert_eq!(b.interaction_id(), Some("sidebar_toggle"));
    }
}
