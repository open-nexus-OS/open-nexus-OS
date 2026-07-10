// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Skeleton` / `SkeletonText` — the design-system loading placeholders
//! (handoff `Skeleton`, `SkeletonText`): dimmed glass blocks sized to the
//! loading content; `circle` for avatars; `SkeletonText` stacks lines with
//! the last one shortened. The shimmer highlight is the motion `phase`
//! (0–100 sweep position) — the motion system advances it; a static build
//! renders one frame. Pure `LayoutNode` builders. DSL-emittable.

extern crate alloc;

use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    Position, Rgba8, ShapeKind, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};

/// Base translucency of the placeholder block.
const BASE_ALPHA: u8 = 46;
/// Shimmer highlight translucency (drawn over the base at `phase`).
const SHIMMER_ALPHA: u8 = 26;
/// Shimmer band width as a fraction of the block width (percent).
const SHIMMER_PERCENT: i32 = 30;

/// A loading placeholder block.
#[derive(Debug, Clone)]
pub struct Skeleton {
    width: FxPx,
    height: FxPx,
    radius: Option<FxPx>,
    circle: bool,
    /// Shimmer sweep position 0–100 (motion-system driven).
    phase: u32,
    id: Option<&'static str>,
}

impl Default for Skeleton {
    fn default() -> Self {
        Self {
            width: FxPx::new(200),
            height: FxPx::new(16),
            radius: None,
            circle: false,
            phase: 0,
            id: None,
        }
    }
}

impl Skeleton {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn width(mut self, px: i32) -> Self {
        self.width = FxPx::new(px.max(4));
        self
    }

    pub fn height(mut self, px: i32) -> Self {
        self.height = FxPx::new(px.max(4));
        self
    }

    /// Corner radius (ignored for `circle`); defaults to the small radius.
    pub fn radius(mut self, px: i32) -> Self {
        self.radius = Some(FxPx::new(px.max(0)));
        self
    }

    /// Round avatar placeholder: width follows height.
    pub fn circle(mut self) -> Self {
        self.circle = true;
        self
    }

    /// Shimmer sweep position 0–100 (motion-system driven).
    pub fn phase(mut self, phase: u32) -> Self {
        self.phase = phase.min(100);
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let base_c = tokens.color(ColorToken::OnSurface);
        let block = Rgba8::new(base_c.r, base_c.g, base_c.b, BASE_ALPHA);
        let shimmer = Rgba8::new(base_c.r, base_c.g, base_c.b, SHIMMER_ALPHA);
        let w = if self.circle { self.height } else { self.width };
        let radius = if self.circle {
            FxPx::new(w.0 / 2)
        } else {
            self.radius.unwrap_or_else(|| tokens.length(LengthToken::RadiusSmall))
        };
        let band_w = (w.0 * SHIMMER_PERCENT / 100).max(4);
        let band_left = (w.0 - band_w) * self.phase as i32 / 100;
        let shimmer_node = LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: Some(FxPx::new(band_w)),
                max_width: Some(FxPx::new(band_w)),
                min_height: Some(self.height),
                max_height: Some(self.height),
                item: FlexItem {
                    position: Position::Absolute,
                    margin: EdgeInsets { left: FxPx::new(band_left), ..EdgeInsets::zero() },
                    ..FlexItem::default()
                },
            },
            VisualStyle {
                background: Some(shimmer),
                corner_radius: CornerRadius::uniform(radius),
                ..VisualStyle::default()
            },
            alloc::vec![],
        );
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: Some(w),
                max_width: Some(w),
                min_height: Some(self.height),
                max_height: Some(self.height),
                item: FlexItem::default(),
            },
            VisualStyle {
                background: Some(block),
                shape: if self.circle { ShapeKind::Circle } else { ShapeKind::Rect },
                corner_radius: CornerRadius::uniform(radius),
                ..VisualStyle::default()
            },
            alloc::vec![shimmer_node],
        )
    }
}

/// A paragraph placeholder: `lines` skeleton rows, the last one shortened.
#[derive(Debug, Clone)]
pub struct SkeletonText {
    lines: u32,
    width: FxPx,
    line_height: FxPx,
    gap: FxPx,
    id: Option<&'static str>,
}

impl Default for SkeletonText {
    fn default() -> Self {
        Self {
            lines: 3,
            width: FxPx::new(240),
            line_height: FxPx::new(14),
            gap: FxPx::new(8),
            id: None,
        }
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

    pub fn width(mut self, px: i32) -> Self {
        self.width = FxPx::new(px.max(16));
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
        let mut rows = alloc::vec::Vec::with_capacity(self.lines as usize);
        for i in 0..self.lines {
            let last = i + 1 == self.lines;
            // The final line is visibly shorter (handoff: paragraph shape).
            let w = if last && self.lines > 1 { self.width.0 * 60 / 100 } else { self.width.0 };
            rows.push(
                Skeleton::new()
                    .width(w)
                    .height(self.line_height.0)
                    .build(tokens),
            );
        }
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Column,
                gap: self.gap,
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
            rows,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn circle_is_square_with_half_radius() {
        let node = Skeleton::new().height(48).circle().build(&BaseTokens);
        match node {
            LayoutNode::Stack(s, v, _) => {
                assert_eq!(s.min_width, s.min_height);
                assert_eq!(v.corner_radius.top_left, FxPx::new(24));
            }
            _ => panic!("skeleton root must be a stack"),
        }
    }

    #[test]
    fn shimmer_band_sweeps_with_phase() {
        let at = |p: u32| match Skeleton::new().width(200).phase(p).build(&BaseTokens) {
            LayoutNode::Stack(_, _, children) => children[0].item().margin.left,
            _ => panic!(),
        };
        assert_eq!(at(0), FxPx::ZERO);
        assert!(at(100) > at(50));
    }

    #[test]
    fn paragraph_shortens_the_last_line() {
        let node = SkeletonText::new().lines(3).width(200).build(&BaseTokens);
        match node {
            LayoutNode::Stack(_, _, rows) => {
                assert_eq!(rows.len(), 3);
                let width = |n: &LayoutNode| match n {
                    LayoutNode::Stack(s, _, _) => s.min_width.unwrap(),
                    _ => panic!(),
                };
                assert_eq!(width(&rows[0]), width(&rows[1]));
                assert!(width(&rows[2]) < width(&rows[1]));
            }
            _ => panic!("skeleton text root must be a stack"),
        }
    }
}
