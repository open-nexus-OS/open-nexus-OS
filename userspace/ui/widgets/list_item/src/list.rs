// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The list group (design `List`): a run of `ListItem` rows with one appearance
//! (insetGrouped · inset · grouped · plain · sidebar), one density and one separator rule; the
//! group carries the corner radius and the surface (glassCard, the inset ring of an
//! insetGrouped group), its first and last rows take the corners. Split out of `lib.rs`.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: src/tests.rs
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeBorder, FxPx, LayoutNode, Material, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, RadiusToken, Tokens, HAIRLINE};

use crate::parts::fade;
use crate::{ListItem, RowSeparator};

/// The group's look (design `List.appearance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListAppearance {
    /// Rounded group on `glassCard` with a 1 px inner `glassEdge` ring (radius 16).
    #[default]
    InsetGrouped,
    /// Rounded group on `glassCard`, no ring.
    Inset,
    /// Edge-to-edge group on `glassCard`.
    Grouped,
    /// No surface.
    Plain,
    /// Navigation rows: 36 px, radius 12 each, 2 px apart, no separators, no surface.
    Sidebar,
}

/// Row density (design `List.density`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListDensity {
    #[default]
    Regular,
    Compact,
}

/// A row's metrics on the 4-pt grid: the body line (20) plus the vertical padding, the minimum
/// height a grid step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowMetrics {
    pub pad_y: i32,
    pub pad_x: i32,
    pub min_height: i32,
}

impl RowMetrics {
    /// The metrics of `appearance` at `density`: regular 12/16 · 48 (sidebar 8/12 · 36),
    /// compact 8/16 · 40 (sidebar 4/12 · 32).
    #[must_use]
    pub const fn of(appearance: ListAppearance, density: ListDensity) -> Self {
        let sidebar = matches!(appearance, ListAppearance::Sidebar);
        let (pad_y, pad_x, min_height) = match (density, sidebar) {
            (ListDensity::Regular, false) => (12, 16, 48),
            (ListDensity::Regular, true) => (8, 12, 36),
            (ListDensity::Compact, false) => (8, 16, 40),
            (ListDensity::Compact, true) => (4, 12, 32),
        };
        Self { pad_y, pad_x, min_height }
    }
}

impl Default for RowMetrics {
    fn default() -> Self {
        Self::of(ListAppearance::InsetGrouped, ListDensity::Regular)
    }
}

/// A group of rows.
#[derive(Debug, Clone, Default)]
pub struct List {
    rows: Vec<ListItem>,
    appearance: ListAppearance,
    density: ListDensity,
    separators: Option<RowSeparator>,
    on_pane: bool,
    id: Option<&'static str>,
}

impl List {
    pub fn new(rows: Vec<ListItem>) -> Self {
        Self { rows, ..Self::default() }
    }
    pub fn appearance(mut self, appearance: ListAppearance) -> Self {
        self.appearance = appearance;
        self
    }
    pub fn density(mut self, density: ListDensity) -> Self {
        self.density = density;
        self
    }
    /// The separator rule (default: inset, none for `Sidebar`).
    pub fn separators(mut self, separators: RowSeparator) -> Self {
        self.separators = Some(separators);
        self
    }
    /// The group sits on an opaque pane or overlay: `glassCard` would vanish there, so the group
    /// takes `fill` at 50 % instead (the design's re-tint inside `.nx-pane` / `.nx-overlay`).
    pub fn on_pane(mut self, on_pane: bool) -> Self {
        self.on_pane = on_pane;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The group's corner radius (16 for the inset looks, none edge to edge).
    #[must_use]
    pub fn radius(&self) -> FxPx {
        match self.appearance {
            ListAppearance::InsetGrouped | ListAppearance::Inset => RadiusToken::Lg.px(),
            _ => FxPx::ZERO,
        }
    }

    fn surface(&self, tokens: &dyn Tokens) -> VisualStyle {
        let fill = if self.on_pane {
            fade(tokens.color(ColorToken::Fill), 50)
        } else {
            tokens.color(ColorToken::GlassCard)
        };
        let mut visual = VisualStyle {
            corner_radius: CornerRadius::uniform(self.radius()),
            ..VisualStyle::default()
        };
        match self.appearance {
            ListAppearance::InsetGrouped => {
                visual.background = Some(fill);
                visual.border = EdgeBorder::all(HAIRLINE, tokens.color(ColorToken::GlassEdge));
                visual.material = Some(Material::Card);
            }
            ListAppearance::Inset | ListAppearance::Grouped => {
                visual.background = Some(fill);
                visual.material = Some(Material::Card);
            }
            ListAppearance::Plain | ListAppearance::Sidebar => {}
        }
        visual
    }

    /// Build the group: a column of the rows on the group surface.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let sidebar = matches!(self.appearance, ListAppearance::Sidebar);
        let separator = self.separators.unwrap_or(if sidebar {
            RowSeparator::None
        } else {
            RowSeparator::Inset
        });
        let metrics = RowMetrics::of(self.appearance, self.density);
        let r = self.radius();
        let last = self.rows.len().saturating_sub(1);
        let surface = self.surface(tokens);
        let rows: Vec<LayoutNode> = self
            .rows
            .into_iter()
            .enumerate()
            .map(|(i, row)| {
                // The first and last cells take the group's corners (the hover layer inherits
                // them); a sidebar row is its own 12 px pill.
                let corners = if sidebar {
                    CornerRadius::uniform(RadiusToken::Md.px())
                } else {
                    CornerRadius {
                        top_left: if i == 0 { r } else { FxPx::ZERO },
                        top_right: if i == 0 { r } else { FxPx::ZERO },
                        bottom_right: if i == last { r } else { FxPx::ZERO },
                        bottom_left: if i == last { r } else { FxPx::ZERO },
                    }
                };
                row.metrics(metrics)
                    .separator(if i == 0 { RowSeparator::None } else { separator })
                    .corner_radius(corners)
                    .build(tokens)
            })
            .collect();
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Column,
                gap: FxPx::new(if sidebar { 2 } else { 0 }),
                align: Align::Stretch,
                ..Stack::default()
            },
            surface,
            rows,
        )
    }
}
