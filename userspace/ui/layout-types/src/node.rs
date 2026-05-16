// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::string::String;
use alloc::vec::Vec;
use crate::border::VisualStyle;
use crate::direction::{Align, Direction, Justify, Overflow, Position, ZIndex};
use crate::types::{EdgeInsets, FxPx};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fraction(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    pub direction: Direction, pub gap: FxPx, pub padding: EdgeInsets,
    pub align: Align, pub justify: Justify, pub overflow: Overflow,
    pub flex_wrap: bool, pub min_width: Option<FxPx>, pub max_width: Option<FxPx>,
    pub min_height: Option<FxPx>, pub max_height: Option<FxPx>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub columns: Vec<Fraction>, pub gap: FxPx, pub row_gap: Option<FxPx>,
    pub padding: EdgeInsets, pub overflow: Overflow,
    pub min_width: Option<FxPx>, pub max_width: Option<FxPx>,
    pub min_height: Option<FxPx>, pub max_height: Option<FxPx>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spacer { pub flex_grow: u32, pub min_size: Option<FxPx> }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlexItem {
    pub flex_grow: u32, pub flex_shrink: u32, pub align_self: Option<Align>,
    pub margin: EdgeInsets, pub position: Position, pub z_index: ZIndex,
    pub min_width: Option<FxPx>, pub max_width: Option<FxPx>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextContent(pub String);
impl TextContent {
    pub fn new(s: impl Into<String>) -> Self { TextContent(s.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextNode {
    pub content: TextContent, pub max_lines: Option<u32>,
    pub min_width: Option<FxPx>, pub max_width: Option<FxPx>,
}

/// The layout tree with VisualStyle on containers and text nodes.
/// VisualStyle is separate from layout properties — paint-only invalidation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutNode {
    Stack(Stack, VisualStyle, Vec<LayoutNode>),
    Grid(Grid, VisualStyle, Vec<LayoutNode>),
    Spacer(Spacer),
    Text(TextNode, VisualStyle),
}
