// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::border::VisualStyle;
use crate::direction::{Align, Direction, Justify, Overflow, Position, ZIndex};
use crate::text::TextStyle;
use crate::types::{EdgeInsets, FxPx};
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fraction(pub u32);

impl Fraction {
    /// Fully opaque (255).
    pub const OPAQUE: Self = Fraction(255);
    /// Fully transparent (0).
    pub const TRANSPARENT: Self = Fraction(0);

    /// Create a new fraction, clamped to 0..255.
    pub const fn new(value: u32) -> Self {
        Fraction(if value > 255 { 255 } else { value })
    }

    /// Fraction value as u8 (0-255).
    pub const fn as_u8(self) -> u8 {
        (if self.0 > 255 { 255 } else { self.0 }) as u8
    }

    /// Blend factor as (numerator, denominator) for alpha compositing.
    /// Returns (fraction, 256) where 0 = transparent, 256 = opaque.
    /// Used with: `blended = (src * f + dst * (256 - f)) / 256`
    pub const fn blend_factor(self) -> (u32, u32) {
        (if self.0 > 255 { 255 } else { self.0 }, 256)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    pub id: Option<&'static str>,
    pub direction: Direction,
    pub gap: FxPx,
    pub padding: EdgeInsets,
    pub align: Align,
    pub justify: Justify,
    pub overflow: Overflow,
    pub flex_wrap: bool,
    pub min_width: Option<FxPx>,
    pub max_width: Option<FxPx>,
    pub min_height: Option<FxPx>,
    pub max_height: Option<FxPx>,
    pub item: FlexItem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub id: Option<&'static str>,
    pub columns: Vec<Fraction>,
    pub gap: FxPx,
    pub row_gap: Option<FxPx>,
    pub padding: EdgeInsets,
    pub overflow: Overflow,
    pub min_width: Option<FxPx>,
    pub max_width: Option<FxPx>,
    pub min_height: Option<FxPx>,
    pub max_height: Option<FxPx>,
    pub item: FlexItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spacer {
    pub id: Option<&'static str>,
    pub flex_grow: u32,
    pub min_size: Option<FxPx>,
    pub item: FlexItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlexItem {
    pub flex_grow: u32,
    pub flex_shrink: u32,
    pub align_self: Option<Align>,
    pub margin: EdgeInsets,
    pub position: Position,
    pub z_index: ZIndex,
    pub min_width: Option<FxPx>,
    pub max_width: Option<FxPx>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextContent(pub String);
impl TextContent {
    pub fn new(s: impl Into<String>) -> Self {
        TextContent(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextNode {
    pub id: Option<&'static str>,
    pub content: TextContent,
    pub style: TextStyle,
    pub item: FlexItem,
    pub max_lines: Option<u32>,
    pub min_width: Option<FxPx>,
    pub max_width: Option<FxPx>,
}

/// A text input node with editable content, caret position, and focus state.
/// Behaves like `TextNode` for layout (fixed height based on font_size).
/// Keyboard events are routed to the focused `TextInput` via `windowd` → `inputd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInputNode {
    pub id: Option<&'static str>,
    pub content: TextContent,
    pub cursor_pos: usize,
    pub placeholder: Option<TextContent>,
    pub max_length: Option<u32>,
    pub style: TextStyle,
    pub item: FlexItem,
    pub min_width: Option<FxPx>,
    pub max_width: Option<FxPx>,
}

impl Default for TextInputNode {
    fn default() -> Self {
        Self {
            id: None,
            content: TextContent::new(""),
            cursor_pos: 0,
            placeholder: None,
            max_length: None,
            style: TextStyle::default(),
            item: FlexItem::default(),
            min_width: None,
            max_width: None,
        }
    }
}

/// The layout tree with VisualStyle on containers and text nodes.
/// VisualStyle is separate from layout properties — paint-only invalidation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutNode {
    Stack(Stack, VisualStyle, Vec<LayoutNode>),
    Grid(Grid, VisualStyle, Vec<LayoutNode>),
    Spacer(Spacer),
    Text(TextNode, VisualStyle),
    TextInput(TextInputNode, VisualStyle),
}

impl Default for FlexItem {
    fn default() -> Self {
        Self {
            flex_grow: 0,
            flex_shrink: 1,
            align_self: None,
            margin: EdgeInsets::zero(),
            position: Position::Relative,
            z_index: 0,
            min_width: None,
            max_width: None,
        }
    }
}

impl Stack {
    pub fn item(&self) -> &FlexItem {
        &self.item
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self {
            id: None,
            direction: Direction::Column,
            gap: FxPx::ZERO,
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
        }
    }
}

impl Grid {
    pub fn item(&self) -> &FlexItem {
        &self.item
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            id: None,
            columns: Vec::new(),
            gap: FxPx::ZERO,
            row_gap: None,
            padding: EdgeInsets::zero(),
            overflow: Overflow::Visible,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            item: FlexItem::default(),
        }
    }
}

impl Default for Spacer {
    fn default() -> Self {
        Self { id: None, flex_grow: 1, min_size: None, item: FlexItem::default() }
    }
}

impl Default for TextNode {
    fn default() -> Self {
        Self {
            id: None,
            content: TextContent::new(""),
            style: TextStyle::default(),
            item: FlexItem::default(),
            max_lines: None,
            min_width: None,
            max_width: None,
        }
    }
}

impl LayoutNode {
    pub fn id(&self) -> Option<&'static str> {
        match self {
            LayoutNode::Stack(stack, _, _) => stack.id,
            LayoutNode::Grid(grid, _, _) => grid.id,
            LayoutNode::Spacer(spacer) => spacer.id,
            LayoutNode::Text(text, _) => text.id,
            LayoutNode::TextInput(input, _) => input.id,
        }
    }

    pub fn item(&self) -> &FlexItem {
        match self {
            LayoutNode::Stack(stack, _, _) => &stack.item,
            LayoutNode::Grid(grid, _, _) => &grid.item,
            LayoutNode::Spacer(spacer) => &spacer.item,
            LayoutNode::Text(text, _) => &text.item,
            LayoutNode::TextInput(input, _) => &input.item,
        }
    }

    pub fn min_width(&self) -> Option<FxPx> {
        match self {
            LayoutNode::Stack(stack, _, _) => stack.min_width.or(stack.item.min_width),
            LayoutNode::Grid(grid, _, _) => grid.min_width.or(grid.item.min_width),
            LayoutNode::Spacer(spacer) => spacer.min_size.or(spacer.item.min_width),
            LayoutNode::Text(text, _) => text.min_width.or(text.item.min_width),
            LayoutNode::TextInput(input, _) => {
                input.min_width.or(input.item.min_width)
            }
        }
    }

    pub fn max_width(&self) -> Option<FxPx> {
        match self {
            LayoutNode::Stack(stack, _, _) => stack.max_width.or(stack.item.max_width),
            LayoutNode::Grid(grid, _, _) => grid.max_width.or(grid.item.max_width),
            LayoutNode::Spacer(spacer) => spacer.item.max_width,
            LayoutNode::Text(text, _) => text.max_width.or(text.item.max_width),
            LayoutNode::TextInput(input, _) => {
                input.max_width.or(input.item.max_width)
            }
        }
    }
}