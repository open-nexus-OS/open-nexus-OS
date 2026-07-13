// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Spinner` — the design-system activity indicator (handoff `Spinner`):
//! twelve tapered spokes fading around the circle (the classic activity
//! model). A pure builder producing a fixed-size `LayoutNode` whose spokes
//! are twelve absolutely-positioned `ShapeKind::Vector` children with a
//! per-spoke alpha ramp — rotation is a paint-time phase shift (the motion
//! system advances `phase`; a static build renders the resting frame).
//! DSL-emittable; tokens resolve the spoke color.

extern crate alloc;

use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    PathPoint, PathShape, Position, Rgba8, ShapeKind, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, Tokens};

/// Twelve tapered spokes in the shared `0..1000` viewbox (precomputed —
/// `no_std` has no trig): spoke 0 points up, successive spokes rotate 30°.
const SPOKES: [[(u16, u16); 4]; 12] = [
    [(466, 220), (448, 30), (552, 30), (534, 220)],
    [(611, 241), (690, 67), (780, 119), (669, 275)],
    [(725, 331), (881, 220), (933, 310), (759, 389)],
    [(780, 466), (970, 448), (970, 552), (780, 534)],
    [(759, 611), (933, 690), (881, 780), (725, 669)],
    [(669, 725), (780, 881), (690, 933), (611, 759)],
    [(534, 780), (552, 970), (448, 970), (466, 780)],
    [(389, 759), (310, 933), (220, 881), (331, 725)],
    [(275, 669), (119, 780), (67, 690), (241, 611)],
    [(220, 534), (30, 552), (30, 448), (220, 466)],
    [(241, 389), (67, 310), (119, 220), (275, 331)],
    [(331, 275), (220, 119), (310, 67), (389, 241)],
];

/// Minimum alpha of the faintest (trailing) spoke; the leading spoke is 255.
const TAIL_ALPHA: u16 = 64;

/// An activity indicator.
#[derive(Debug, Clone)]
pub struct Spinner {
    size: FxPx,
    color: ColorToken,
    /// Leading-spoke index (0..12) — the motion system advances this each
    /// tick to rotate the fade around the circle.
    phase: usize,
    /// All spokes opaque (the host paints the rotating fade). See [`flat`].
    ///
    /// [`flat`]: Spinner::flat
    flat: bool,
    id: Option<&'static str>,
}

impl Default for Spinner {
    fn default() -> Self {
        Self { size: FxPx::new(28), color: ColorToken::OnSurface, phase: 0, flat: false, id: None }
    }
}

impl Spinner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Diameter in px (handoff `size`; default 28).
    pub fn size(mut self, px: i32) -> Self {
        self.size = FxPx::new(px.max(8));
        self
    }

    /// Spoke color token (defaults to `OnSurface` — the glass text primary).
    pub fn color(mut self, color: ColorToken) -> Self {
        self.color = color;
        self
    }

    /// Animation phase: which spoke leads the fade (wraps at 12).
    pub fn phase(mut self, phase: usize) -> Self {
        self.phase = phase % SPOKES.len();
        self
    }

    /// Build every spoke fully opaque (no baked resting fade): for hosts that
    /// animate the fade themselves as a per-spoke paint-time opacity wash (the
    /// DSL carousel loop) — a multiplicative wash over the baked fade would
    /// double-fade the tail.
    pub fn flat(mut self) -> Self {
        self.flat = true;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Alpha of spoke `i` for the current phase: the leading spoke is fully
    /// opaque, trailing spokes fade linearly down to [`TAIL_ALPHA`].
    fn spoke_alpha(&self, i: usize) -> u8 {
        let n = SPOKES.len();
        // Distance BEHIND the leading spoke (0 = leading).
        let d = (i + n - self.phase) % n;
        let a = 255u16 - (255 - TAIL_ALPHA) * d as u16 / (n as u16 - 1);
        a as u8
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let base = tokens.color(self.color);
        let d = Some(self.size);
        let mut spokes = alloc::vec::Vec::with_capacity(SPOKES.len());
        for (i, quad) in SPOKES.iter().enumerate() {
            let shape = PathShape::polygon(&[
                PathPoint::new(quad[0].0, quad[0].1),
                PathPoint::new(quad[1].0, quad[1].1),
                PathPoint::new(quad[2].0, quad[2].1),
                PathPoint::new(quad[3].0, quad[3].1),
            ]);
            let alpha = if self.flat { 255 } else { self.spoke_alpha(i) };
            let color = Rgba8::new(base.r, base.g, base.b, alpha);
            spokes.push(LayoutNode::Stack(
                Stack {
                    id: None,
                    direction: Direction::Row,
                    gap: FxPx::ZERO,
                    padding: EdgeInsets::zero(),
                    align: Align::Center,
                    justify: Justify::Center,
                    overflow: Overflow::Visible,
                    flex_wrap: false,
                    min_width: d,
                    max_width: d,
                    min_height: d,
                    max_height: d,
                    item: FlexItem { position: Position::Absolute, ..FlexItem::default() },
                },
                VisualStyle {
                    background: Some(color),
                    shape: ShapeKind::Vector(alloc::vec![shape]),
                    corner_radius: CornerRadius::uniform(FxPx::ZERO),
                    ..VisualStyle::default()
                },
                alloc::vec![],
            ));
        }
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            spokes,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn twelve_spokes_with_monotonic_fade() {
        let s = Spinner::new();
        // Leading spoke opaque, tail dimmest, strictly monotonic in between.
        assert_eq!(s.spoke_alpha(0), 255);
        for i in 1..12 {
            assert!(s.spoke_alpha(i) < s.spoke_alpha(i - 1));
        }
        assert!(u16::from(s.spoke_alpha(11)) >= TAIL_ALPHA);
    }

    #[test]
    fn phase_rotates_the_leading_spoke() {
        let s = Spinner::new().phase(3);
        assert_eq!(s.spoke_alpha(3), 255);
        assert!(s.spoke_alpha(2) < s.spoke_alpha(3));
    }

    #[test]
    fn builds_container_with_twelve_absolute_children() {
        let node = Spinner::new().size(20).build(&BaseTokens);
        match node {
            LayoutNode::Stack(_, _, children) => {
                assert_eq!(children.len(), 12);
                for c in &children {
                    assert_eq!(c.item().position, Position::Absolute);
                }
            }
            _ => panic!("spinner root must be a stack"),
        }
    }
}
