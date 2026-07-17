// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `TreeView` — the design-system hierarchical tree (handoff `TreeView`):
//! collapsible nodes with indentation, disclosure chevrons, optional icons, and
//! a selected-row accent. A pure builder producing a flattened `LayoutNode::Stack`
//! column (rows for expanded descendants). The disclosure uses the built-in
//! triangle shapes (▼ expanded · ▲ collapsed — a right-pointing ▶ needs the SVG
//! icon primitive). DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    ShapeKind, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};
use nexus_widget_text::Text;

/// Indent per depth level (px).
const INDENT: i32 = 16;

/// A tree node.
#[derive(Debug, Clone)]
pub struct TreeNode {
    id: &'static str,
    label: String,
    icon: Option<LayoutNode>,
    children: Vec<TreeNode>,
    expanded: bool,
}

impl TreeNode {
    /// A leaf node.
    pub fn leaf(id: &'static str, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), icon: None, children: Vec::new(), expanded: false }
    }
    /// A branch node with children.
    pub fn branch(id: &'static str, label: impl Into<String>, children: Vec<TreeNode>) -> Self {
        Self { id, label: label.into(), icon: None, children, expanded: false }
    }
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }
}

/// A hierarchical tree.
#[derive(Debug, Clone, Default)]
pub struct TreeView {
    nodes: Vec<TreeNode>,
    selected: Option<&'static str>,
    id: Option<&'static str>,
}

impl TreeView {
    pub fn new(nodes: Vec<TreeNode>) -> Self {
        Self { nodes, ..Self::default() }
    }
    pub fn selected(mut self, id: &'static str) -> Self {
        self.selected = Some(id);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn disclosure(tokens: &dyn Tokens, expanded: bool) -> LayoutNode {
        let visual = VisualStyle {
            background: Some(tokens.color(ColorToken::OnSurfaceVariant)),
            shape: if expanded { ShapeKind::TriangleDown } else { ShapeKind::TriangleUp },
            corner_radius: CornerRadius::uniform(FxPx::ZERO),
            ..VisualStyle::default()
        };
        Self::fixed(visual, 10, 6)
    }

    fn spacer_box() -> LayoutNode {
        Self::fixed(VisualStyle::default(), 10, 6)
    }

    fn fixed(visual: VisualStyle, w: i32, h: i32) -> LayoutNode {
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
                min_width: Some(FxPx::new(w)),
                max_width: Some(FxPx::new(w)),
                min_height: Some(FxPx::new(h)),
                max_height: Some(FxPx::new(h)),
                item: FlexItem::default(),
            },
            visual,
            alloc::vec![],
        )
    }

    fn push_node(
        &self,
        tokens: &dyn Tokens,
        node: TreeNode,
        depth: i32,
        out: &mut Vec<LayoutNode>,
    ) {
        let selected = self.selected == Some(node.id);
        let has_children = !node.children.is_empty();
        let mut style = Style::new().rounded(tokens.length(LengthToken::RadiusSmall));
        if selected {
            style = style.background(tokens.color(ColorToken::SurfaceVariant));
        }
        let color = if selected { ColorToken::Accent } else { ColorToken::OnSurface };

        let mut row: Vec<LayoutNode> = Vec::new();
        if has_children {
            row.push(Self::disclosure(tokens, node.expanded));
        } else {
            row.push(Self::spacer_box());
        }
        if let Some(icon) = node.icon {
            row.push(icon);
        }
        row.push(Text::new(node.label).color(color).build(tokens));

        out.push(LayoutNode::Stack(
            Stack {
                id: Some(node.id),
                direction: Direction::Row,
                gap: FxPx::new(6),
                padding: EdgeInsets {
                    top: FxPx::new(6),
                    right: FxPx::new(8),
                    bottom: FxPx::new(6),
                    left: FxPx::new(8 + depth * INDENT),
                },
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
            style.visual(),
            row,
        ));

        if node.expanded {
            for child in node.children {
                self.push_node(tokens, child, depth + 1, out);
            }
        }
    }

    /// Build the tree node (flattened rows for visible/expanded nodes).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let mut rows: Vec<LayoutNode> = Vec::new();
        for node in self.nodes.clone() {
            self.push_node(tokens, node, 0, &mut rows);
        }
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Column,
                gap: FxPx::new(1),
                padding: EdgeInsets::zero(),
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: Some(FxPx::new(200)),
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            rows,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn expanded_branch_flattens_children_collapsed_hides_them() {
        let t = BaseTokens;
        let expanded = TreeView::new(alloc::vec![TreeNode::branch(
            "src",
            "src",
            alloc::vec![TreeNode::leaf("app", "app.rs"), TreeNode::leaf("lib", "lib.rs")],
        )
        .expanded(true)])
        .build(&t);
        match expanded {
            // branch row + 2 child rows = 3.
            LayoutNode::Stack(_, _, rows) => assert_eq!(rows.len(), 3),
            _ => panic!(),
        }
        let collapsed = TreeView::new(alloc::vec![TreeNode::branch(
            "src",
            "src",
            alloc::vec![TreeNode::leaf("app", "app.rs")],
        )])
        .build(&t);
        match collapsed {
            LayoutNode::Stack(_, _, rows) => assert_eq!(rows.len(), 1, "collapsed hides children"),
            _ => panic!(),
        }
    }

    #[test]
    fn selected_row_tints_accent_and_carries_id() {
        let t = BaseTokens;
        match TreeView::new(alloc::vec![TreeNode::leaf("app", "app.rs")]).selected("app").build(&t)
        {
            LayoutNode::Stack(_, _, rows) => match &rows[0] {
                LayoutNode::Stack(s, v, _) => {
                    assert_eq!(s.id, Some("app"));
                    assert_eq!(v.background, Some(t.color(ColorToken::SurfaceVariant)));
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }
}
