// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `TextArea` — the design-system multiline input (handoff `TextArea`): a label,
//! a bordered glass box sized for `rows`, and a footer with helper/error text
//! and an optional character counter. A pure builder producing a
//! `LayoutNode::Stack` column. v1 renders the value as multiline text (the app
//! owns the value; a dedicated multiline edit primitive is a follow-up).
//! DSL-emittable.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, LineHeight,
    Overflow, Spacer, Stack, TextAlign, TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
};
use nexus_style::{InteractionState, Style};
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens, TypographyToken};

/// Approximate line box height (px) at body size.
const LINE: i32 = 20;

/// A multiline text input.
#[derive(Debug, Clone)]
pub struct TextArea {
    label: Option<String>,
    value: String,
    placeholder: Option<String>,
    helper: Option<String>,
    error: Option<String>,
    rows: u32,
    max_length: Option<u32>,
    show_count: bool,
    state: InteractionState,
    id: Option<&'static str>,
}

impl Default for TextArea {
    fn default() -> Self {
        Self {
            label: None,
            value: String::new(),
            placeholder: None,
            helper: None,
            error: None,
            rows: 3,
            max_length: None,
            show_count: false,
            state: InteractionState::Default,
            id: None,
        }
    }
}

impl TextArea {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
    pub fn helper(mut self, helper: impl Into<String>) -> Self {
        self.helper = Some(helper.into());
        self
    }
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }
    pub fn rows(mut self, rows: u32) -> Self {
        self.rows = rows.max(1);
        self
    }
    /// Character limit; enables the counter denominator.
    pub fn max_length(mut self, max: u32) -> Self {
        self.max_length = Some(max);
        self
    }
    /// Show an `x/limit` counter (requires `max_length`).
    pub fn show_count(mut self, show: bool) -> Self {
        self.show_count = show;
        self
    }
    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn caption(tokens: &dyn Tokens, text: String, color: ColorToken) -> LayoutNode {
        LayoutNode::Text(
            TextNode {
                id: None,
                content: TextContent::new(text),
                style: TextStyle {
                    font_size: tokens.type_size(TypographyToken::Sm),
                    font_weight: FontWeight::Regular,
                    line_height: LineHeight::Relative(FxPx::new(150)),
                    text_align: TextAlign::Left,
                    color: tokens.color(color),
                    white_space: WhiteSpace::NoWrap,
                },
                item: FlexItem::default(),
                max_lines: Some(1),
                min_width: None,
                max_width: None,
            },
            VisualStyle::default(),
        )
    }

    fn border_role(&self) -> ColorToken {
        if self.error.is_some() {
            ColorToken::Danger
        } else if self.state.shows_focus_ring() {
            ColorToken::FocusRing
        } else {
            ColorToken::Border
        }
    }

    /// Build the text-area column.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let border = tokens.color(self.border_role());
        let mut box_style = Style::new()
            .background(tokens.color(ColorToken::Surface))
            .rounded(tokens.length(LengthToken::RadiusMedium))
            .border(tokens.length(LengthToken::BorderThin), border);
        if self.state.is_disabled() {
            box_style = box_style.opacity(self.state.opacity());
        }

        // Body: value in primary ink, or placeholder in muted ink.
        let (body_text, body_color) = if self.value.is_empty() {
            (self.placeholder.clone().unwrap_or_default(), ColorToken::OnSurfaceVariant)
        } else {
            (self.value.clone(), ColorToken::OnSurface)
        };
        let body = LayoutNode::Text(
            TextNode {
                id: self.id,
                content: TextContent::new(body_text),
                style: TextStyle {
                    font_size: tokens.type_size(TypographyToken::Base),
                    font_weight: FontWeight::Regular,
                    line_height: LineHeight::Relative(FxPx::new(150)),
                    text_align: TextAlign::Left,
                    color: tokens.color(body_color),
                    white_space: WhiteSpace::Normal,
                },
                item: FlexItem::default(),
                max_lines: Some(self.rows),
                min_width: None,
                max_width: None,
            },
            VisualStyle::default(),
        );
        let field_box = LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Column,
                gap: FxPx::ZERO,
                padding: EdgeInsets::symmetric(FxPx::new(8), FxPx::new(12)),
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: Some(FxPx::new(200)),
                max_width: None,
                min_height: Some(FxPx::new(self.rows as i32 * LINE)),
                max_height: None,
                item: FlexItem::default(),
            },
            box_style.visual(),
            alloc::vec![body],
        );

        // Footer: helper/error on the left, counter on the right.
        let left = self
            .error
            .clone()
            .map(|e| Self::caption(tokens, e, ColorToken::Danger))
            .or_else(|| self.helper.clone().map(|h| Self::caption(tokens, h, ColorToken::OnSurfaceVariant)));
        let counter = (self.show_count && self.max_length.is_some()).then(|| {
            let n = self.value.chars().count();
            Self::caption(
                tokens,
                format!("{}/{}", n, self.max_length.unwrap_or(0)),
                ColorToken::OnSurfaceVariant,
            )
        });

        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(label) = self.label.clone() {
            col.push(Self::caption(tokens, label, ColorToken::OnSurfaceVariant));
        }
        col.push(field_box);
        if left.is_some() || counter.is_some() {
            let mut footer: Vec<LayoutNode> = Vec::new();
            if let Some(l) = left {
                footer.push(l);
            }
            footer.push(LayoutNode::Spacer(Spacer {
                id: None,
                flex_grow: 1,
                min_size: Some(FxPx::new(8)),
                item: FlexItem::default(),
            }));
            if let Some(c) = counter {
                footer.push(c);
            }
            col.push(LayoutNode::Stack(
                Stack {
                    id: None,
                    direction: Direction::Row,
                    gap: FxPx::new(8),
                    padding: EdgeInsets::zero(),
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
                footer,
            ));
        }

        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Column,
                gap: FxPx::new(4),
                padding: EdgeInsets::zero(),
                align: Align::Start,
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
            col,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn rows_size_the_box_and_error_adds_footer() {
        let t = BaseTokens;
        let ta = TextArea::new().label("Notiz").rows(5).error("Pflichtfeld");
        assert_eq!(ta.border_role(), ColorToken::Danger);
        match ta.build(&t) {
            LayoutNode::Stack(_, _, col) => {
                assert_eq!(col.len(), 3, "label + box + footer");
                // box min height = rows * LINE.
                match &col[1] {
                    LayoutNode::Stack(s, _, _) => {
                        assert_eq!(s.min_height, Some(FxPx::new(5 * LINE)))
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }

    #[test]
    fn counter_requires_show_count_and_max_length() {
        let t = BaseTokens;
        // No counter without show_count.
        match TextArea::new().value("hi").max_length(280).build(&t) {
            LayoutNode::Stack(_, _, col) => assert_eq!(col.len(), 1, "just the box"),
            _ => panic!(),
        }
        // With both → footer appears.
        match TextArea::new().value("hi").max_length(280).show_count(true).build(&t) {
            LayoutNode::Stack(_, _, col) => assert_eq!(col.len(), 2, "box + footer"),
            _ => panic!(),
        }
    }
}
