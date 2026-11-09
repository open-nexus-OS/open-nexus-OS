// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `ListItem` + `List` — the design artifacts' list row and its group (RFC-0116).
//!
//! A row is one line — [selection circle 22] [leading] [title + subtitle] [trailing] [check]
//! [badge] [chevron 16] [reorder handle] — and optional content below it, aligned to the text
//! column. The title is body 14/20 medium `label` (`danger` when destructive, the leading glyph
//! with it), the subtitle footnote 12/16 `labelSecondary`, a text trailing value body
//! `labelSecondary`; the row pads 12/16 to a 48 px minimum (60 with a subtitle). A group's
//! separator is the hairline at the TOP of the text column, from the text x to the trailing
//! edge — the row draws it as the top edge of its text column, so it follows whatever leading
//! the row has. `selected` is the persistent `fill` layer, `disabled` dims the row to .45.
//!
//! [`List`] gives its rows the appearance's metrics and corners and draws the group surface.
//! Pure builders producing `LayoutNode::Stack`s; the swipe actions, the reorder drag and the
//! motion are runtime concerns (gestures and transforms the layout tree does not carry).

extern crate alloc;

mod list;
mod parts;
#[cfg(test)]
mod tests;

pub use list::{List, ListAppearance, ListDensity, RowMetrics};

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Border, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, FxPx,
    Justify, LayoutNode, Stack, VisualStyle,
};
use nexus_style::state::Dim;
use nexus_theme_tokens::{ColorToken, TextStyleToken, Tokens, HAIRLINE};
use nexus_widget_icon::with_ink;
use nexus_widget_text::Text;

/// The line's column gap (between leading, text, trailing) and the row gap below it.
const COLUMN_GAP: i32 = 12;
const ROW_GAP: i32 = 8;

/// Where a row draws its separator hairline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowSeparator {
    /// None: a standalone row, the first row of a group, a sidebar list.
    #[default]
    None,
    /// From the text column to the trailing edge (the group default).
    Inset,
    /// Across the whole row.
    Full,
}

/// How a selectable row shows its selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionStyle {
    /// A leading 22 px circle: a ring at rest, an accent disc with a check when selected.
    #[default]
    Circle,
    /// A trailing 16 px accent check on the selected row.
    Checkmark,
}

#[derive(Debug, Clone)]
enum Trailing {
    /// A control or any caller node, as built.
    Node(Box<LayoutNode>),
    /// A value: body `labelSecondary`, one line, shrinking before the title does.
    Text(String),
}

/// A list / settings row.
#[derive(Debug, Clone)]
pub struct ListItem {
    title: String,
    subtitle: Option<String>,
    leading: Option<LayoutNode>,
    trailing: Option<Trailing>,
    show_chevron: bool,
    destructive: bool,
    badge: Option<String>,
    badge_tint: ColorToken,
    selected: bool,
    selectable: bool,
    selection_style: SelectionStyle,
    reorderable: bool,
    disabled: bool,
    children: Vec<LayoutNode>,
    separator: RowSeparator,
    metrics: RowMetrics,
    corners: CornerRadius,
    id: Option<&'static str>,
}

impl Default for ListItem {
    fn default() -> Self {
        Self {
            title: String::new(),
            subtitle: None,
            leading: None,
            trailing: None,
            show_chevron: false,
            destructive: false,
            badge: None,
            badge_tint: ColorToken::Danger,
            selected: false,
            selectable: false,
            selection_style: SelectionStyle::Circle,
            reorderable: false,
            disabled: false,
            children: Vec::new(),
            separator: RowSeparator::None,
            metrics: RowMetrics::default(),
            corners: CornerRadius::none(),
            id: None,
        }
    }
}

impl ListItem {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), ..Self::default() }
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
    /// The leading node (an 18 px icon, a 30 px avatar), centred on the line. A glyph or text
    /// leading takes the title's ink when the row is destructive.
    pub fn leading(mut self, leading: LayoutNode) -> Self {
        self.leading = Some(leading);
        self
    }
    /// A trailing control (toggle, select, badge node), as built.
    pub fn trailing(mut self, trailing: LayoutNode) -> Self {
        self.trailing = Some(Trailing::Node(Box::new(trailing)));
        self
    }
    /// A trailing value ("Verbunden", "42 %"): body `labelSecondary`, one line, shrinking with
    /// an ellipsis before the title gives up its room.
    pub fn trailing_text(mut self, value: impl Into<String>) -> Self {
        self.trailing = Some(Trailing::Text(value.into()));
        self
    }
    /// The disclosure chevron (`chevron-right` 16, `labelTertiary`).
    pub fn show_chevron(mut self, show: bool) -> Self {
        self.show_chevron = show;
        self
    }
    /// The title and a glyph leading in `danger`.
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }
    /// A count badge (20 px capsule, `danger` unless [`Self::badge_tint`] says otherwise).
    pub fn badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }
    pub fn badge_tint(mut self, tint: ColorToken) -> Self {
        self.badge_tint = tint;
        self
    }
    /// The persistent `fill` layer of the current item (a sidebar-style highlight). A
    /// selectable row shows its selection by circle or check instead.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    /// Multi-selection: the row carries a selection circle (or check) driven by `selected`.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }
    pub fn selection_style(mut self, style: SelectionStyle) -> Self {
        self.selection_style = style;
        self
    }
    /// The trailing reorder handle (the drag itself is the runtime's).
    pub fn reorderable(mut self, reorderable: bool) -> Self {
        self.reorderable = reorderable;
        self
    }
    /// Dimmed to .45 and inert.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Content below the line (a slider, a chip row), aligned to the text column, 8 px under it.
    pub fn children(mut self, children: Vec<LayoutNode>) -> Self {
        self.children = children;
        self
    }
    /// The separator hairline (a [`List`] sets it on every row but the first).
    pub fn separator(mut self, separator: RowSeparator) -> Self {
        self.separator = separator;
        self
    }
    /// Padding and minimum height (a [`List`] sets its appearance's and density's).
    pub fn metrics(mut self, metrics: RowMetrics) -> Self {
        self.metrics = metrics;
        self
    }
    /// The cell's corners (a [`List`] gives its first and last rows the group's), so the
    /// selected layer stays inside the group's shape.
    pub fn corner_radius(mut self, corners: CornerRadius) -> Self {
        self.corners = corners;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Whether the row draws the disclosure chevron.
    pub fn wants_chevron(&self) -> bool {
        self.show_chevron
    }

    fn title_column(
        tokens: &dyn Tokens,
        title: String,
        subtitle: Option<String>,
        ink: ColorToken,
    ) -> LayoutNode {
        let mut col: Vec<LayoutNode> =
            alloc::vec![Text::new(title).weight(FontWeight::Medium).color(ink).build(tokens)];
        if let Some(subtitle) = subtitle {
            col.push(
                Text::new(subtitle)
                    .text_style(TextStyleToken::Footnote)
                    .color(ColorToken::LabelSecondary)
                    .build(tokens),
            );
        }
        LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle::default(),
            col,
        )
    }

    /// Build the list-row node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let ListItem {
            title,
            subtitle,
            leading,
            trailing,
            show_chevron,
            destructive,
            badge,
            badge_tint,
            selected,
            selectable,
            selection_style,
            reorderable,
            disabled,
            children,
            separator,
            metrics: m,
            corners,
            id,
        } = self;
        let ink = if destructive { ColorToken::Danger } else { ColorToken::Label };
        let two_lines = subtitle.is_some();

        // The line: text column + everything trailing.
        let mut line: Vec<LayoutNode> =
            alloc::vec![Self::title_column(tokens, title, subtitle, ink)];
        match trailing {
            Some(Trailing::Node(node)) => line.push(*node),
            Some(Trailing::Text(value)) => {
                line.push(Text::new(value).color(ColorToken::LabelSecondary).build(tokens));
            }
            None => {}
        }
        if selectable && selection_style == SelectionStyle::Checkmark {
            line.push(parts::trailing_check(tokens, selected));
        }
        if let Some(text) = badge {
            line.push(parts::badge(tokens, text, badge_tint));
        }
        if show_chevron {
            line.push(parts::chevron(tokens));
        }
        if reorderable {
            line.push(parts::reorder_handle(tokens));
        }
        let line = LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                gap: FxPx::new(COLUMN_GAP),
                align: Align::Center,
                ..Stack::default()
            },
            VisualStyle::default(),
            line,
        );

        // The text column spans from the text x to the trailing edge: it carries the vertical
        // and trailing padding, the content below the line and the inset separator.
        let below = !children.is_empty();
        let mut text_col = alloc::vec![line];
        text_col.extend(children);
        let text_col = LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                gap: FxPx::new(ROW_GAP),
                padding: EdgeInsets {
                    top: FxPx::new(m.pad_y),
                    right: FxPx::new(m.pad_x),
                    bottom: FxPx::new(m.pad_y),
                    left: FxPx::ZERO,
                },
                align: Align::Stretch,
                justify: Justify::Center,
                item: FlexItem {
                    flex_grow: 1,
                    align_self: Some(Align::Stretch),
                    ..FlexItem::default()
                },
                ..Stack::default()
            },
            VisualStyle {
                border: hairline_if(tokens, separator == RowSeparator::Inset),
                ..VisualStyle::default()
            },
            text_col,
        );

        // The leading parts sit on the line; with content below, they keep to the line's
        // centre instead of the whole row's.
        let line_h = if two_lines { 36 } else { 20 };
        let on_line = |node: LayoutNode| -> LayoutNode {
            if !below {
                return node;
            }
            let h = node.min_width().map_or(line_h, |w| w.0);
            with_top_margin(node, m.pad_y + (line_h - h).max(0) / 2)
        };
        let mut row: Vec<LayoutNode> = Vec::new();
        if selectable && selection_style == SelectionStyle::Circle {
            row.push(on_line(parts::selection_circle(tokens, selected)));
        }
        if let Some(node) = leading {
            let node = if destructive { with_ink(node, tokens.color(ink)) } else { node };
            row.push(on_line(node));
        }
        row.push(text_col);

        let highlight = selected && !selectable;
        LayoutNode::Stack(
            Stack {
                id,
                direction: Direction::Row,
                gap: FxPx::new(COLUMN_GAP),
                padding: EdgeInsets { left: FxPx::new(m.pad_x), ..EdgeInsets::zero() },
                align: if below { Align::Start } else { Align::Center },
                min_height: Some(FxPx::new(m.min_height)),
                ..Stack::default()
            },
            VisualStyle {
                background: highlight.then(|| tokens.color(ColorToken::Fill)),
                corner_radius: corners,
                // A list row dims with the value controls (0.45).
                opacity: disabled.then_some(Dim::Control.opacity()),
                border: hairline_if(tokens, separator == RowSeparator::Full),
                ..VisualStyle::default()
            },
            row,
        )
    }
}

/// The separator: a hairline along the top edge, or no edge.
fn hairline_if(tokens: &dyn Tokens, on: bool) -> EdgeBorder {
    if !on {
        return EdgeBorder::none();
    }
    let line = Border { width: HAIRLINE, color: tokens.color(ColorToken::Separator) };
    EdgeBorder { top: Some(line), ..EdgeBorder::none() }
}

/// `node` pushed `top` px down from the start of its cross axis.
fn with_top_margin(mut node: LayoutNode, top: i32) -> LayoutNode {
    let item = match &mut node {
        LayoutNode::Stack(s, _, _) => &mut s.item,
        LayoutNode::Grid(g, _, _) => &mut g.item,
        LayoutNode::Text(t, _) => &mut t.item,
        LayoutNode::TextInput(t, _) => &mut t.item,
        LayoutNode::Spacer(s) => &mut s.item,
    };
    item.margin.top = FxPx::new(top);
    node
}
