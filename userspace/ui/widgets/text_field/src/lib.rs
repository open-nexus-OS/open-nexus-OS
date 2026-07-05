// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `TextField` — a styled single-line text input.
//!
//! A pure builder producing a `LayoutNode::TextInput`. Visual chrome comes from
//! a [`Style`] (background/border/rounded), text appearance from a `TextStyle`.
//! Like every widget it holds no input handler: the `id` is the interaction id —
//! the compositor focuses/routes keystrokes by it and the app owns the value
//! (framework/app split). `value`/`cursor` are props pushed in by the app, so
//! the widget stays pure and DSL-emittable.

extern crate alloc;

use alloc::string::String;
use nexus_layout_types::{FlexItem, LayoutNode, TextContent, TextInputNode, TextStyle};
use nexus_style::Style;

mod glass_text_field;
pub use glass_text_field::{FieldSize, GlassTextField};

/// A styled single-line text input.
#[derive(Debug, Clone, Default)]
pub struct TextField {
    id: Option<&'static str>,
    style: Style,
    text_style: TextStyle,
    value: String,
    placeholder: Option<String>,
    max_length: Option<u32>,
}

impl TextField {
    pub fn new() -> Self {
        Self::default()
    }

    /// Interaction id — the compositor focuses/routes input by it; the app owns the value.
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Visual chrome (background/border/rounded/shadow).
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Text appearance (font/size/color).
    pub fn text_style(mut self, text_style: TextStyle) -> Self {
        self.text_style = text_style;
        self
    }

    /// Current value (app-owned prop). The cursor is placed at the end.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// Placeholder shown when empty.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Maximum input length.
    pub fn max_length(mut self, max: u32) -> Self {
        self.max_length = Some(max);
        self
    }

    /// The field's interaction id.
    pub fn interaction_id(&self) -> Option<&'static str> {
        self.id
    }

    /// Build the layout-node.
    pub fn build(self) -> LayoutNode {
        let cursor_pos = self.value.chars().count();
        LayoutNode::TextInput(
            TextInputNode {
                id: self.id,
                content: TextContent::new(self.value),
                cursor_pos,
                placeholder: self.placeholder.map(TextContent::new),
                max_length: self.max_length,
                style: self.text_style,
                item: FlexItem::default(),
                min_width: None,
                max_width: None,
            },
            self.style.visual(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{FxPx, Rgba8};

    #[test]
    fn text_field_builds_text_input_with_value_and_cursor() {
        let node = TextField::new()
            .id("search")
            .style(Style::new().background(Rgba8::new(30, 30, 36, 255)).rounded(FxPx::new(8)))
            .value("héllo") // 5 chars (accented) — cursor counts chars, not bytes
            .placeholder("type to filter…")
            .max_length(20)
            .build();
        match node {
            LayoutNode::TextInput(input, visual) => {
                assert_eq!(input.id, Some("search"));
                assert_eq!(input.content.as_str(), "héllo");
                assert_eq!(input.cursor_pos, 5);
                assert!(input.placeholder.is_some());
                assert_eq!(input.max_length, Some(20));
                assert!(visual.background.is_some());
            }
            _ => panic!("TextField must build a TextInput"),
        }
    }

    #[test]
    fn text_field_exposes_interaction_id() {
        let f = TextField::new().id("name_field");
        assert_eq!(f.interaction_id(), Some("name_field"));
    }
}
