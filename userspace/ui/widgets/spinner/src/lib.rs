// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `Spinner` — the design-system activity indicator (design artifacts `Spinner`):
//! twelve constant-width, round-capped spokes in a 100-unit box (stroke `max(7.5, 200/size)`,
//! from `r = 20 + sw/2` to `48 − sw/2`), spoke `i` at 30° steps with opacity `(i+1)/12`, in
//! `labelSecondary` unless tinted. A pure builder producing a fixed-size `LayoutNode` whose
//! spokes are twelve absolutely-positioned `ShapeKind::Stroke` children — the rotation is a
//! paint-time step (the host's carousel loop, or `phase` for a static frame).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; tests/ui_v10_goldens (spinner_*, refresher_*)
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use nexus_layout_types::{
    Align, FlexItem, FxPx, Justify, LayoutNode, PathPoint, PathShape, Position, Rgba8, ShapeKind,
    Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, ControlSize, Tokens};

/// Number of spokes.
pub const SPOKES: usize = 12;

/// `(sin, cos) × 10 000` of the spoke directions, clockwise from 12 o'clock.
const SPOKE_DIR: [(i16, i16); SPOKES] = [
    (0, 10000),
    (5000, 8660),
    (8660, 5000),
    (10000, 0),
    (8660, -5000),
    (5000, -8660),
    (0, -10000),
    (-5000, -8660),
    (-8660, -5000),
    (-10000, 0),
    (-8660, 5000),
    (-5000, 8660),
];

/// The diameter of a control size: 14 / 20 / 28 / 36 / 44.
pub fn diameter(size: ControlSize) -> i32 {
    match size {
        ControlSize::Xs => 14,
        ControlSize::Sm => 20,
        ControlSize::Md => 28,
        ControlSize::Lg => 36,
        ControlSize::Xl => 44,
    }
}

/// An activity indicator.
#[derive(Debug, Clone)]
pub struct Spinner {
    size: FxPx,
    color: ColorToken,
    /// Rotation step (0..12) — the motion system advances it; a static build is step 0.
    phase: usize,
    /// All spokes opaque (the host paints the rotating fade). See [`flat`](Spinner::flat).
    flat: bool,
    id: Option<&'static str>,
}

impl Default for Spinner {
    fn default() -> Self {
        Self {
            size: FxPx::new(diameter(ControlSize::Md)),
            color: ColorToken::LabelSecondary,
            phase: 0,
            flat: false,
            id: None,
        }
    }
}

impl Spinner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Diameter in px (design `size` as a number).
    pub fn size(mut self, px: i32) -> Self {
        self.size = FxPx::new(px.max(8));
        self
    }
    /// Diameter of a control size (design `size` as xs … xl).
    pub fn control_size(self, size: ControlSize) -> Self {
        self.size(diameter(size))
    }
    /// The tint (default `labelSecondary`).
    pub fn tint(self, tint: ColorToken) -> Self {
        self.color(tint)
    }
    /// The spoke colour role (legacy name of [`tint`](Self::tint)).
    pub fn color(mut self, color: ColorToken) -> Self {
        self.color = color;
        self
    }
    /// Rotation step: the glyph turned `phase × 30°` clockwise (wraps at 12).
    pub fn phase(mut self, phase: usize) -> Self {
        self.phase = phase % SPOKES;
        self
    }
    /// Build every spoke fully opaque (no baked fade): for hosts that animate the fade
    /// themselves as a per-spoke paint-time opacity (the DSL carousel loop) — a
    /// multiplicative fade over a baked one would double-fade the tail.
    pub fn flat(mut self) -> Self {
        self.flat = true;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The stroke width in thousandths of the box: `max(7.5, 200/size)` of 100 (≥ 2 px).
    pub fn stroke_milli(&self) -> i32 {
        let px = self.size.0.max(1);
        ((2000 + px / 2) / px).max(75)
    }

    /// Alpha of the spoke drawn at position `i`: the design spoke `(i − phase) mod 12`, whose
    /// opacity is `(spoke + 1) / 12`.
    fn spoke_alpha(&self, i: usize) -> u8 {
        let spoke = (i + SPOKES - self.phase) % SPOKES;
        ((spoke as u32 + 1) * 255 / SPOKES as u32) as u8
    }

    /// The centre line of spoke `i`: from `20 + sw/2` to `48 − sw/2` (of 100) off centre.
    fn spoke_path(&self, i: usize) -> PathShape {
        let sw = self.stroke_milli();
        let (r_in, r_out) = (200 + sw / 2, 480 - sw / 2);
        let (sin, cos) = SPOKE_DIR[i];
        let at = |r: i32| {
            let x = 500 + r * i32::from(sin) / 10_000;
            let y = 500 - r * i32::from(cos) / 10_000;
            PathPoint::new(x.clamp(0, 1000) as u16, y.clamp(0, 1000) as u16)
        };
        PathShape { points: alloc::vec![at(r_in), at(r_out)], closed: false }
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let base = tokens.color(self.color);
        let d = Some(self.size);
        let width_milli = self.stroke_milli() as u16;
        let spokes = (0..SPOKES)
            .map(|i| {
                let alpha = if self.flat { 255 } else { self.spoke_alpha(i) };
                let color = Rgba8::new(
                    base.r,
                    base.g,
                    base.b,
                    (u32::from(base.a) * u32::from(alpha) / 255) as u8,
                );
                LayoutNode::Stack(
                    Stack {
                        min_width: d,
                        max_width: d,
                        min_height: d,
                        max_height: d,
                        item: FlexItem { position: Position::Absolute, ..FlexItem::default() },
                        ..Stack::default()
                    },
                    VisualStyle {
                        background: Some(color),
                        shape: ShapeKind::Stroke {
                            paths: alloc::vec![self.spoke_path(i)],
                            width_milli,
                        },
                        ..VisualStyle::default()
                    },
                    alloc::vec::Vec::new(),
                )
            })
            .collect();
        LayoutNode::Stack(
            Stack {
                id: self.id,
                align: Align::Center,
                justify: Justify::Center,
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
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
    fn spoke_opacity_rises_to_the_last_spoke() {
        let s = Spinner::new();
        assert_eq!(s.spoke_alpha(0), 21, "1/12");
        assert_eq!(s.spoke_alpha(11), 255, "12/12");
        for i in 1..SPOKES {
            assert!(s.spoke_alpha(i) > s.spoke_alpha(i - 1));
        }
    }

    #[test]
    fn phase_turns_the_glyph_clockwise() {
        // One step: the opaque spoke moves from 11 to 0 (one 30° step clockwise).
        let s = Spinner::new().phase(1);
        assert_eq!(s.spoke_alpha(0), 255);
        assert_eq!(s.spoke_alpha(1), 21);
    }

    #[test]
    fn spokes_are_constant_width_round_capped_lines() {
        let s = Spinner::new();
        assert_eq!(s.stroke_milli(), 75, "7.5 of 100 at 28 px (2.1 px)");
        assert_eq!(Spinner::new().size(14).stroke_milli(), 143, "≥ 2 px at 14");
        // Spoke 0 points up: from 6.7 px to 12.4 px off centre at 28 px.
        let p = s.spoke_path(0);
        assert_eq!((p.points[0].x_milli, p.points[0].y_milli), (500, 263));
        assert_eq!((p.points[1].x_milli, p.points[1].y_milli), (500, 57));
    }

    #[test]
    fn sizes_and_colour() {
        let t = BaseTokens;
        let node = Spinner::new().control_size(ControlSize::Xl).build(&t);
        match &node {
            LayoutNode::Stack(s, _, kids) => {
                assert_eq!(s.min_width, Some(FxPx::new(44)));
                assert_eq!(kids.len(), SPOKES);
                for c in kids {
                    assert_eq!(c.item().position, Position::Absolute);
                    match c {
                        LayoutNode::Stack(_, v, _) => {
                            assert!(matches!(v.shape, ShapeKind::Stroke { width_milli: 75, .. }));
                            let base = t.color(ColorToken::LabelSecondary);
                            assert!(v
                                .background
                                .is_some_and(|b| (b.r, b.g, b.b) == (base.r, base.g, base.b)));
                        }
                        _ => panic!("spoke"),
                    }
                }
            }
            _ => panic!("spinner root must be a stack"),
        }
        let flat = Spinner::new().flat().tint(ColorToken::Accent).build(&t);
        match &flat {
            LayoutNode::Stack(_, _, kids) => match &kids[0] {
                LayoutNode::Stack(_, v, _) => {
                    assert_eq!(v.background, Some(t.color(ColorToken::Accent)))
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }
}
