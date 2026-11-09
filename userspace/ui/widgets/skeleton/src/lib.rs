// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `Skeleton` / `SkeletonText` — the design-system loading placeholders (design
//! artifacts `Skeleton`, `SkeletonText`): a block in the control fill, as wide as its container
//! (14 px high, radius md) or a `circle`, with the shimmer band in `fillHover` as its first
//! child — the host's sweep loop moves it (`phase` places it in a static frame of a pinned
//! block). The design's band is a full-width horizontal gradient; the painter has vertical
//! gradients only, so the band is a solid 30 % strip. `SkeletonText` stacks `lines` bars of
//! 12 px, the last at 60 %. Pure `LayoutNode` builders. DSL-emittable.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; tests/ui_v10_goldens (skeleton_*)
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, LayoutNode, Overflow, ShapeKind,
    Spacer, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, RadiusToken, Tokens};

/// Shimmer band width as a share of the block (percent).
const SHIMMER_PERCENT: u32 = 30;

/// `[part, rest]` of a row split by flex growth on a zero basis (exact at any width).
fn share(part: LayoutNode, grow: u32, rest: u32) -> [LayoutNode; 2] {
    let mut part = part;
    if let LayoutNode::Stack(s, _, _) = &mut part {
        s.item.flex_grow = grow;
        s.item.flex_basis = Some(FxPx::ZERO);
    }
    let spacer = LayoutNode::Spacer(Spacer {
        flex_grow: rest,
        item: FlexItem { flex_grow: rest, flex_basis: Some(FxPx::ZERO), ..FlexItem::default() },
        ..Spacer::default()
    });
    [part, spacer]
}

/// A loading placeholder block.
#[derive(Debug, Clone)]
pub struct Skeleton {
    width: Option<FxPx>,
    height: FxPx,
    radius: Option<FxPx>,
    circle: bool,
    /// Shimmer position 0–100 on a pinned block (a static frame).
    phase: u32,
    id: Option<&'static str>,
}

impl Default for Skeleton {
    fn default() -> Self {
        Self { width: None, height: FxPx::new(14), radius: None, circle: false, phase: 0, id: None }
    }
}

impl Skeleton {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pin the width (px); by default the block is as wide as its container.
    pub fn width(mut self, px: i32) -> Self {
        self.width = Some(FxPx::new(px.max(4)));
        self
    }
    /// Height (px, default 14; a circle's diameter).
    pub fn height(mut self, px: i32) -> Self {
        self.height = FxPx::new(px.max(4));
        self
    }
    /// Corner radius (ignored for `circle`); defaults to radius md.
    pub fn radius(mut self, px: i32) -> Self {
        self.radius = Some(FxPx::new(px.max(0)));
        self
    }
    /// A round placeholder (an avatar): a square of `height`.
    pub fn circle(mut self) -> Self {
        self.circle = true;
        self
    }
    /// Shimmer position 0–100 on a pinned block (motion-system driven otherwise).
    pub fn phase(mut self, phase: u32) -> Self {
        self.phase = phase.min(100);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The block's width: pinned, the circle's diameter, or `None` (the container's).
    fn block_width(&self) -> Option<FxPx> {
        if self.circle {
            Some(self.height)
        } else {
            self.width
        }
    }

    /// The shimmer band — the block's FIRST child (the host's sweep loop translates it).
    fn band(&self, tokens: &dyn Tokens) -> Vec<LayoutNode> {
        let h = Some(self.height);
        let band = |item: FlexItem, width: Option<FxPx>| {
            LayoutNode::Stack(
                Stack {
                    min_width: width,
                    max_width: width,
                    min_height: h,
                    max_height: h,
                    item,
                    ..Stack::default()
                },
                VisualStyle {
                    background: Some(tokens.color(ColorToken::FillHover)),
                    ..VisualStyle::default()
                },
                Vec::new(),
            )
        };
        match self.block_width() {
            Some(w) => {
                let band_w = (w.0 * SHIMMER_PERCENT as i32 / 100).max(4);
                let left = (w.0 - band_w) * self.phase as i32 / 100;
                let item = FlexItem {
                    margin: EdgeInsets { left: FxPx::new(left), ..EdgeInsets::zero() },
                    flex_shrink: 0,
                    ..FlexItem::default()
                };
                alloc::vec![band(item, Some(FxPx::new(band_w)))]
            }
            None => share(band(FlexItem::default(), None), SHIMMER_PERCENT, 100 - SHIMMER_PERCENT)
                .into(),
        }
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let w = self.block_width();
        let radius = if self.circle {
            RadiusToken::Full.px()
        } else {
            self.radius.unwrap_or_else(|| RadiusToken::Md.px())
        };
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                align: Align::Center,
                overflow: Overflow::Hidden,
                min_width: w,
                max_width: w,
                min_height: Some(self.height),
                max_height: Some(self.height),
                ..Stack::default()
            },
            VisualStyle {
                background: Some(tokens.color(ColorToken::Fill)),
                shape: if self.circle { ShapeKind::Circle } else { ShapeKind::Rect },
                corner_radius: CornerRadius::uniform(radius),
                ..VisualStyle::default()
            },
            self.band(tokens),
        )
    }
}

/// A paragraph placeholder: `lines` bars of 12 px, the last one at 60 %.
#[derive(Debug, Clone)]
pub struct SkeletonText {
    lines: u32,
    width: Option<FxPx>,
    line_height: FxPx,
    gap: FxPx,
    id: Option<&'static str>,
}

impl Default for SkeletonText {
    fn default() -> Self {
        Self { lines: 3, width: None, line_height: FxPx::new(12), gap: FxPx::new(8), id: None }
    }
}

impl SkeletonText {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lines(mut self, lines: u32) -> Self {
        self.lines = lines.clamp(1, 16);
        self
    }
    /// Pin the width (px); by default the paragraph is as wide as its container.
    pub fn width(mut self, px: i32) -> Self {
        self.width = Some(FxPx::new(px.max(16)));
        self
    }
    pub fn gap(mut self, px: i32) -> Self {
        self.gap = FxPx::new(px.max(0));
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let line = |width: Option<i32>| {
            let mut sk = Skeleton::new().height(self.line_height.0);
            if let Some(w) = width {
                sk = sk.width(w);
            }
            sk.build(tokens)
        };
        let rows = (0..self.lines)
            .map(|i| {
                let last = i + 1 == self.lines && self.lines > 1;
                match (last, self.width) {
                    (false, w) => line(w.map(|w| w.0)),
                    (true, Some(w)) => line(Some(w.0 * 60 / 100)),
                    // The last line at 60 % of an unpinned paragraph: a 3 : 2 split.
                    (true, None) => LayoutNode::Stack(
                        Stack { direction: Direction::Row, ..Stack::default() },
                        VisualStyle::default(),
                        share(line(None), 3, 2).into(),
                    ),
                }
            })
            .collect();
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Column,
                gap: self.gap,
                align: Align::Stretch,
                min_width: self.width,
                max_width: self.width,
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
    use nexus_theme_tokens::BaseTokens;

    fn stack(node: &LayoutNode) -> (&Stack, &VisualStyle, &Vec<LayoutNode>) {
        match node {
            LayoutNode::Stack(s, v, k) => (s, v, k),
            _ => panic!("expected a stack"),
        }
    }

    #[test]
    fn the_block_fills_its_container_in_the_control_fill() {
        let t = BaseTokens;
        let node = Skeleton::new().build(&t);
        let (s, v, kids) = stack(&node);
        assert_eq!((s.min_width, s.max_width), (None, None), "100 % of the container");
        assert_eq!(s.max_height, Some(FxPx::new(14)));
        assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
        assert_eq!(v.corner_radius.top_left, FxPx::new(12), "radius md");
        // The band is the FIRST child: 30 % on a zero basis, in fillHover.
        let (band, bv, _) = stack(&kids[0]);
        assert_eq!(band.item.flex_grow, 30);
        assert_eq!(bv.background, Some(t.color(ColorToken::FillHover)));
    }

    #[test]
    fn circle_is_a_square_of_its_height() {
        let node = Skeleton::new().height(48).circle().build(&BaseTokens);
        let (s, v, _) = stack(&node);
        assert_eq!(s.min_width, Some(FxPx::new(48)));
        assert_eq!(s.min_width, s.min_height);
        assert_eq!(v.shape, ShapeKind::Circle);
    }

    #[test]
    fn a_pinned_block_places_the_band_at_phase() {
        let at = |p: u32| match Skeleton::new().width(200).phase(p).build(&BaseTokens) {
            LayoutNode::Stack(_, _, children) => children[0].item().margin.left,
            _ => panic!(),
        };
        assert_eq!(at(0), FxPx::ZERO);
        assert_eq!(at(100), FxPx::new(140));
        assert!(at(100) > at(50));
    }

    #[test]
    fn paragraph_lines_are_12_high_and_the_last_is_60_percent() {
        let t = BaseTokens;
        let node = SkeletonText::new().build(&t);
        let (col, _, rows) = stack(&node);
        assert_eq!(rows.len(), 3);
        assert_eq!(col.gap, FxPx::new(8));
        assert_eq!(stack(&rows[0]).0.max_height, Some(FxPx::new(12)));
        // The last row splits 3 : 2.
        let (_, _, split) = stack(&rows[2]);
        assert_eq!(split[0].item().flex_grow, 3);
        // Pinned: 60 % in px.
        let node = SkeletonText::new().lines(2).width(200).build(&t);
        let (_, _, rows) = stack(&node);
        assert_eq!(stack(&rows[1]).0.min_width, Some(FxPx::new(120)));
    }
}
