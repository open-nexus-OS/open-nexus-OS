// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Pagination` and `PageControl` — the design artifacts' page navigators (RFC-0116).
//!
//! [`Pagination`] (design `Pagination`): 36 px glass circles for previous / next (`glassCard`,
//! 1 px `glassEdge`, the lit top edge, a 15 px chevron at stroke 2.2, dimmed to .40 at the
//! ends) around 36 px page capsules (min 36 wide, 0 8 padding, body medium `label` on the same
//! glass); the current page is the accent capsule, `onAccent` semibold with the glow under it.
//! Gaps are 28 px ellipses; the trail always truncates: 1 … [page ± siblings] … count.
//!
//! [`PageControl`] (design `PageControl`): the page dots, the current one stretched into a
//! capsule (`label` at 92 %, the others at 30 %), sized xs–xl, optionally on a `fill` capsule.
//! Pure builders producing `LayoutNode::Stack` rows; the stretch motion is the runtime's.

extern crate alloc;

mod page_control;
#[cfg(test)]
mod tests;

pub use page_control::PageControl;

use alloc::format;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, BoxShadow, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, FxPx,
    Justify, LayoutNode, Stack, VisualStyle,
};
use nexus_style::state::Dim;
use nexus_theme_tokens::{ColorToken, RadiusToken, ShadowToken, Tokens, HAIRLINE};
use nexus_widget_icon::{Icon, LucideSymbol};
use nexus_widget_text::Text;

/// The controls' height (the md control height).
const H: i32 = 36;

/// A page navigator (1-based).
#[derive(Debug, Clone)]
pub struct Pagination {
    count: u32,
    page: u32,
    sibling: u32,
    page_ids: Vec<&'static str>,
    arrow_ids: Option<(&'static str, &'static str)>,
    id: Option<&'static str>,
}

impl Default for Pagination {
    fn default() -> Self {
        Self { count: 1, page: 1, sibling: 1, page_ids: Vec::new(), arrow_ids: None, id: None }
    }
}

impl Pagination {
    pub fn new(count: u32) -> Self {
        Self { count: count.max(1), ..Self::default() }
    }

    pub fn page(mut self, page: u32) -> Self {
        self.page = page.clamp(1, self.count);
        self
    }
    /// Pages shown on each side of the current page before truncating.
    pub fn sibling(mut self, sibling: u32) -> Self {
        self.sibling = sibling;
        self
    }
    /// The page buttons' node ids (their tap targets): index 0 is page 1.
    pub fn page_ids(mut self, ids: Vec<&'static str>) -> Self {
        self.page_ids = ids;
        self
    }
    /// The previous / next buttons' node ids.
    pub fn arrow_ids(mut self, prev: &'static str, next: &'static str) -> Self {
        self.arrow_ids = Some((prev, next));
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The visible page slots: `Some(n)` = a page, `None` = an ellipsis gap. Always
    /// 1 … [page − siblings … page + siblings] … count (7 pages at page 1: 1 2 … 7).
    pub fn visible_pages(&self) -> Vec<Option<u32>> {
        let (count, page, sib) = (self.count, self.page, self.sibling);
        let left = page.saturating_sub(sib).max(2);
        let right = page.saturating_add(sib).min(count.saturating_sub(1));
        let mut out = alloc::vec![Some(1)];
        if left > 2 {
            out.push(None);
        }
        out.extend((left..=right).map(Some));
        if right < count.saturating_sub(1) {
            out.push(None);
        }
        if count > 1 {
            out.push(Some(count));
        }
        out
    }

    /// The glass of a resting button: `glassCard`, 1 px `glassEdge`, the lit top edge.
    fn glass(tokens: &dyn Tokens) -> VisualStyle {
        VisualStyle {
            background: Some(tokens.color(ColorToken::GlassCard)),
            border: EdgeBorder::all(HAIRLINE, tokens.color(ColorToken::GlassEdge)),
            inset_highlight: Some(tokens.shadow(ShadowToken::EdgeHighlight).color),
            corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
            ..VisualStyle::default()
        }
    }

    fn button(
        id: Option<&'static str>,
        min_w: i32,
        pad_x: i32,
        visual: VisualStyle,
        child: LayoutNode,
    ) -> LayoutNode {
        let h = Some(FxPx::new(H));
        LayoutNode::Stack(
            Stack {
                id,
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(pad_x)),
                align: Align::Center,
                justify: Justify::Center,
                min_width: Some(FxPx::new(min_w)),
                min_height: h,
                max_height: h,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            visual,
            alloc::vec![child],
        )
    }

    fn arrow(
        tokens: &dyn Tokens,
        next: bool,
        enabled: bool,
        id: Option<&'static str>,
    ) -> LayoutNode {
        let symbol = if next { LucideSymbol::ChevronRight } else { LucideSymbol::ChevronLeft };
        let glyph =
            Icon::lucide(symbol).stroke_tenths(22).size(15).color(ColorToken::Label).build(tokens);
        let mut visual = Self::glass(tokens);
        if !enabled {
            visual.opacity = Some(Dim::Option.opacity());
        }
        let mut node = Self::button(id, H, 0, visual, glyph);
        if let LayoutNode::Stack(s, _, _) = &mut node {
            s.max_width = Some(FxPx::new(H));
        }
        node
    }

    fn page_button(
        tokens: &dyn Tokens,
        n: u32,
        current: bool,
        id: Option<&'static str>,
    ) -> LayoutNode {
        let (ink, weight) = if current {
            (ColorToken::OnAccent, FontWeight::Semibold)
        } else {
            (ColorToken::Label, FontWeight::Medium)
        };
        let label = Text::new(format!("{n}")).weight(weight).color(ink).build(tokens);
        let visual = if current {
            VisualStyle {
                background: Some(tokens.color(ColorToken::Accent)),
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                shadow: Some(BoxShadow {
                    offset_x: FxPx::ZERO,
                    offset_y: FxPx::new(2),
                    blur_radius: FxPx::new(8),
                    spread: FxPx::ZERO,
                    color: tokens.color(ColorToken::AccentGlow),
                }),
                ..VisualStyle::default()
            }
        } else {
            Self::glass(tokens)
        };
        // 8 px padding inside the design's 1 px edge (transparent on the current page).
        Self::button(id, H, 9, visual, label)
    }

    fn ellipsis(tokens: &dyn Tokens) -> LayoutNode {
        let w = Some(FxPx::new(28));
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                align: Align::Center,
                justify: Justify::Center,
                min_width: w,
                max_width: w,
                ..Stack::default()
            },
            VisualStyle::default(),
            alloc::vec![Text::new("…").color(ColorToken::LabelSecondary).build(tokens)],
        )
    }

    /// Build the pagination row.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let page = self.page;
        let (prev_id, next_id) = match self.arrow_ids {
            Some((p, n)) => (Some(p), Some(n)),
            None => (None, None),
        };
        let mut row: Vec<LayoutNode> = alloc::vec![Self::arrow(tokens, false, page > 1, prev_id)];
        for slot in self.visible_pages() {
            row.push(match slot {
                Some(n) => {
                    let id = self.page_ids.get((n - 1) as usize).copied();
                    Self::page_button(tokens, n, n == page, id)
                }
                None => Self::ellipsis(tokens),
            });
        }
        row.push(Self::arrow(tokens, true, page < self.count, next_id));

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(4),
                align: Align::Center,
                justify: Justify::Center,
                ..Stack::default()
            },
            VisualStyle::default(),
            row,
        )
    }
}
