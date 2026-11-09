// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The page dots (design `PageControl`): dot and spacing by size (xs 4/4 · sm 6/8 ·
//! md 8/8 · lg 10/12 · xl 12/12), the current dot stretched to d + 1.5 d, `label` (or a tint) at
//! 92 % for the current page and 30 % for the others, a hit height of max(20, d + 12), an
//! optional `fill` capsule with a glass edge behind (`prominent`), the .45 disabled dim. Lives
//! beside `Pagination` (both navigate pages); the stretch motion between pages is the runtime's.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: src/tests.rs
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeBorder, EdgeInsets, FxPx, Justify, LayoutNode, Rgba8,
    Stack, VisualStyle,
};
use nexus_style::state::Dim;
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, Tokens, HAIRLINE};

/// Page dots (0-based pages, like the reference).
#[derive(Debug, Clone, Default)]
pub struct PageControl {
    count: usize,
    page: usize,
    size: ControlSize,
    tint: Option<ColorToken>,
    prominent: bool,
    disabled: bool,
    dot_ids: Vec<&'static str>,
    id: Option<&'static str>,
}

/// `color` at `pct` % of its own alpha.
fn fade(color: Rgba8, pct: u32) -> Rgba8 {
    Rgba8 { a: (u32::from(color.a) * pct / 100) as u8, ..color }
}

impl PageControl {
    pub fn new(count: usize) -> Self {
        Self { count, ..Self::default() }
    }
    /// The current page (0-based, clamped).
    pub fn page(mut self, page: usize) -> Self {
        self.page = page.min(self.count.saturating_sub(1));
        self
    }
    /// The dot size (default `Md`: 8 px dots, 8 px apart).
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
    /// The current dot's colour (default `label`).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }
    /// The dots on a `fill` capsule with a 1 px glass edge.
    pub fn prominent(mut self, prominent: bool) -> Self {
        self.prominent = prominent;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// The dots' node ids (their tap targets), by page.
    pub fn dot_ids(mut self, ids: Vec<&'static str>) -> Self {
        self.dot_ids = ids;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The dot diameter and the spacing between dots.
    #[must_use]
    pub fn dot_metrics(&self) -> (i32, i32) {
        match self.size {
            ControlSize::Xs => (4, 4),
            ControlSize::Sm => (6, 8),
            ControlSize::Md => (8, 8),
            ControlSize::Lg => (10, 12),
            ControlSize::Xl => (12, 12),
        }
    }

    /// Build the dots row (empty for no pages).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let (d, gap) = self.dot_metrics();
        let extra = (d * 3 + 1) / 2;
        let hit = Some(FxPx::new((d + 12).max(20)));
        let label = tokens.color(ColorToken::Label);
        let on = self.tint.map_or(label, |t| tokens.color(t));
        let dots: Vec<LayoutNode> = (0..self.count)
            .map(|i| {
                let current = i == self.page;
                let w = Some(FxPx::new(if current { d + extra } else { d }));
                let dot = LayoutNode::Stack(
                    Stack {
                        min_width: w,
                        max_width: w,
                        min_height: Some(FxPx::new(d)),
                        max_height: Some(FxPx::new(d)),
                        ..Stack::default()
                    },
                    VisualStyle {
                        background: Some(if current { fade(on, 92) } else { fade(label, 30) }),
                        corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                        ..VisualStyle::default()
                    },
                    alloc::vec![],
                );
                LayoutNode::Stack(
                    Stack {
                        id: self.dot_ids.get(i).copied(),
                        direction: Direction::Row,
                        padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(gap / 2)),
                        align: Align::Center,
                        min_height: hit,
                        max_height: hit,
                        ..Stack::default()
                    },
                    VisualStyle {
                        corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                        ..VisualStyle::default()
                    },
                    alloc::vec![dot],
                )
            })
            .collect();
        let mut visual = VisualStyle {
            corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
            opacity: self.disabled.then(|| Dim::Control.opacity()),
            ..VisualStyle::default()
        };
        if self.prominent {
            visual.background = Some(tokens.color(ColorToken::Fill));
            visual.border = EdgeBorder::all(HAIRLINE, tokens.color(ColorToken::GlassEdge));
        }
        let pad = if self.prominent { gap } else { 0 };
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(pad)),
                align: Align::Center,
                justify: Justify::Center,
                ..Stack::default()
            },
            visual,
            dots,
        )
    }
}
