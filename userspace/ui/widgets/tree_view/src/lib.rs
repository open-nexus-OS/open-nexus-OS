// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `TreeView` — the design artifacts' hierarchical tree (design `TreeView`, RFC-0116): a column
//! of 28 px rows, 2 px apart (4 8 padding, 8 + 20 px per level of indent, 6 gap, radius 8). A row
//! is a 16 px disclosure slot — a 12 px `chevron-right` (stroke 2.4, 80 %) that points down when
//! the branch is open — an optional icon at 80 % and the label in body medium `label`; the
//! selected row is the accent pill with `onAccent` semibold (its icon at full strength).
//! Pure builder producing a flattened `LayoutNode::Stack` column (rows for the visible nodes);
//! the open/close motion is the runtime's.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
    Rgba8, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, RadiusToken, Tokens};
use nexus_widget_icon::{with_ink, Icon, LucideSymbol};
use nexus_widget_text::Text;

/// Indent per depth level (px).
const INDENT: i32 = 20;

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
    /// The row's icon; the row inks it with its label (at 80 % unless selected).
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

/// `color` at `pct` % of its own alpha.
fn fade(color: Rgba8, pct: u32) -> Rgba8 {
    Rgba8 { a: (u32::from(color.a) * pct / 100) as u8, ..color }
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

    /// The 16 px disclosure slot: a chevron for a branch (down when open), empty for a leaf.
    fn disclosure(tokens: &dyn Tokens, branch: bool, expanded: bool, ink: Rgba8) -> LayoutNode {
        let mut slot = Vec::new();
        if branch {
            let symbol =
                if expanded { LucideSymbol::ChevronDown } else { LucideSymbol::ChevronRight };
            let glyph = Icon::lucide(symbol).stroke_tenths(24).size(12).build(tokens);
            slot.push(with_ink(glyph, fade(ink, 80)));
        }
        let w = Some(FxPx::new(16));
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                align: Align::Center,
                justify: Justify::Center,
                min_width: w,
                max_width: w,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle::default(),
            slot,
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
        let branch = !node.children.is_empty();
        let (role, weight) = if selected {
            (ColorToken::OnAccent, FontWeight::Semibold)
        } else {
            (ColorToken::Label, FontWeight::Medium)
        };
        let ink = tokens.color(role);

        let mut row: Vec<LayoutNode> =
            alloc::vec![Self::disclosure(tokens, branch, node.expanded, ink)];
        if let Some(icon) = node.icon {
            row.push(with_ink(icon, if selected { ink } else { fade(ink, 80) }));
        }
        let mut label = Text::new(node.label).weight(weight).color(role).build(tokens);
        if let LayoutNode::Text(t, _) = &mut label {
            t.item.flex_grow = 1;
        }
        row.push(label);

        out.push(LayoutNode::Stack(
            Stack {
                id: Some(node.id),
                direction: Direction::Row,
                gap: FxPx::new(6),
                padding: EdgeInsets {
                    top: FxPx::new(4),
                    right: FxPx::new(8),
                    bottom: FxPx::new(4),
                    left: FxPx::new(8 + depth * INDENT),
                },
                align: Align::Center,
                ..Stack::default()
            },
            VisualStyle {
                background: selected.then(|| tokens.color(ColorToken::Accent)),
                corner_radius: CornerRadius::uniform(RadiusToken::Sm.px()),
                ..VisualStyle::default()
            },
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
                gap: FxPx::new(2),
                align: Align::Stretch,
                ..Stack::default()
            },
            VisualStyle::default(),
            rows,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::ShapeKind;
    use nexus_text_baked::measure_text::BakedTextMeasure;
    use nexus_theme_tokens::BaseTokens;

    fn tree() -> TreeView {
        TreeView::new(alloc::vec![TreeNode::branch(
            "src",
            "src",
            alloc::vec![TreeNode::leaf("app", "app.rs"), TreeNode::leaf("lib", "lib.rs")],
        )
        .expanded(true)])
        .selected("app")
        .id("tree")
    }

    fn row(node: &LayoutNode, i: usize) -> (&Stack, &VisualStyle, &[LayoutNode]) {
        match node {
            LayoutNode::Stack(_, _, rows) => match &rows[i] {
                LayoutNode::Stack(s, v, c) => (s, v, c),
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn expanded_branch_flattens_children_collapsed_hides_them() {
        let t = BaseTokens;
        match tree().build(&t) {
            LayoutNode::Stack(s, _, rows) => {
                assert_eq!((s.gap, s.min_width), (FxPx::new(2), None));
                assert_eq!(rows.len(), 3);
            }
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
    fn rows_are_28_and_indent_20_per_level() {
        let t = BaseTokens;
        let all = LayoutEngine::new()
            .layout(&tree().build(&t), FxPx::new(300), &BakedTextMeasure)
            .expect("layout")
            .boxes;
        let src = all.iter().find(|b| b.id == Some("src")).expect("src");
        let app = all.iter().find(|b| b.id == Some("app")).expect("app");
        assert_eq!(src.rect.height, FxPx::new(28));
        assert_eq!(app.rect.y, src.rect.y + FxPx::new(28 + 2));
        let node = tree().build(&t);
        assert_eq!(row(&node, 0).0.padding.left, FxPx::new(8));
        assert_eq!(row(&node, 1).0.padding.left, FxPx::new(28));
    }

    #[test]
    fn the_selected_row_is_the_accent_pill() {
        let t = BaseTokens;
        let node = tree().build(&t);
        let (s, v, kids) = row(&node, 1);
        assert_eq!(s.id, Some("app"));
        assert_eq!(v.background, Some(t.color(ColorToken::Accent)));
        assert_eq!(v.corner_radius.top_left, FxPx::new(8));
        match kids.last() {
            Some(LayoutNode::Text(n, _)) => {
                assert_eq!(n.style.color, t.color(ColorToken::OnAccent));
                assert_eq!(n.style.font_weight, FontWeight::Semibold);
            }
            _ => panic!(),
        }
        let (_, rest, rest_kids) = row(&node, 2);
        assert_eq!(rest.background, None);
        match rest_kids.last() {
            Some(LayoutNode::Text(n, _)) => {
                assert_eq!((n.style.font_size.0, n.style.font_weight), (14, FontWeight::Medium));
                assert_eq!(n.style.color, t.color(ColorToken::Label));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn the_disclosure_is_a_12_px_chevron_in_a_16_px_slot() {
        let t = BaseTokens;
        let node = tree().build(&t);
        let (_, _, kids) = row(&node, 0);
        match &kids[0] {
            LayoutNode::Stack(slot, _, glyph) => {
                assert_eq!(slot.min_width, Some(FxPx::new(16)));
                match &glyph[0] {
                    LayoutNode::Stack(gs, gv, _) => {
                        assert_eq!(gs.min_width, Some(FxPx::new(12)));
                        assert!(matches!(gv.shape, ShapeKind::Stroke { width_milli: 100, .. }));
                        let label = t.color(ColorToken::Label);
                        assert_eq!(gv.background, Some(fade(label, 80)));
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
        // A leaf keeps the slot empty so labels line up.
        let (_, _, leaf) = row(&node, 2);
        match &leaf[0] {
            LayoutNode::Stack(slot, _, glyph) => {
                assert_eq!(slot.min_width, Some(FxPx::new(16)));
                assert!(glyph.is_empty());
            }
            _ => panic!(),
        }
    }
}
