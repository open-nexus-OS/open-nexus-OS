// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Slider` — the design-system range slider (design_handoff_panels `.slider`):
//! a recessed pill TRACK with a bright FILL growing from the left, and a glyph
//! riding at a fixed inset inside that fill.
//!
//! There is no separate thumb. The fill's own leading edge is the handle, which
//! is why it carries a minimum width — at value 0 the cap must still be visible
//! and grabbable rather than collapsing to a hairline.
//!
//! Width is NOT fixed. The track is a flex child that takes whatever its row
//! grants it, and the fill/rest split is expressed as flex WEIGHTS (value vs
//! 100 − value) rather than pixels — the layout equivalent of the handoff's
//! `width: 70%`. A builder that computed pixels here would have to be told the
//! row width, and every card the slider sits in has a different one.
//!
//! The embedded glyph arrives as a ready-made node (`leading`), so this crate
//! stays free of the icon set: symbol-name resolution is a DSL-layer concern.

extern crate alloc;

use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    Stack, VisualStyle,
};
use nexus_style::InteractionState;
use nexus_theme_tokens::{ColorToken, Tokens};

/// Track height; the radius is half of it, so the track is a true pill.
const TRACK_H: i32 = 28;
const RADIUS: i32 = TRACK_H / 2;
/// The fill never shrinks below this: its leading edge IS the handle, and a
/// handle you cannot see is one you cannot grab.
const MIN_FILL: i32 = 28;
/// Inset of the embedded glyph from the track's leading edge. Fixed — the
/// glyph does not travel with the value, the fill sweeps OVER it.
const GLYPH_INSET: i32 = 8;

/// A horizontal range slider (value 0..=100).
#[derive(Debug, Default)]
pub struct Slider {
    value: u8,
    state: InteractionState,
    id: Option<&'static str>,
    leading: Option<LayoutNode>,
}

impl Slider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Value as a percent 0..=100 (clamped).
    pub fn value(mut self, value: u8) -> Self {
        self.value = value.min(100);
        self
    }

    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The glyph embedded in the fill (brightness sun, volume speaker). Built
    /// by the caller so this crate needs no icon dependency.
    pub fn leading(mut self, node: LayoutNode) -> Self {
        self.leading = Some(node);
        self
    }

    /// The fill's share of the track, as a flex weight. The remainder gets
    /// `100 - share`, so the two always divide the row exactly.
    pub fn fill_weight(&self) -> u32 {
        self.value as u32
    }

    /// A flex bar: `weight` decides its share of the track, `min_w` its floor.
    fn bar(
        weight: u32,
        min_w: Option<i32>,
        visual: VisualStyle,
        padding: EdgeInsets,
        children: alloc::vec::Vec<LayoutNode>,
    ) -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding,
                align: Align::Center,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: min_w.map(FxPx::new),
                max_width: None,
                min_height: Some(FxPx::new(TRACK_H)),
                max_height: Some(FxPx::new(TRACK_H)),
                item: FlexItem { flex_grow: weight, ..FlexItem::default() },
            },
            visual,
            children,
        )
    }

    /// Build the slider node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let weight = self.fill_weight();
        let fill_visual = VisualStyle {
            background: Some(tokens.color(ColorToken::SliderFill)),
            corner_radius: CornerRadius::uniform(FxPx::new(RADIUS)),
            ..VisualStyle::default()
        };
        // The handoff's `inset 0 1px 2px rgba(0,0,0,.30)`. The row-based
        // painter has no offscreen buffer to blur in, so the 2px falloff is
        // not reproducible; the 1px inset LINE is, and it is what actually
        // reads as a groove at this size.
        let track_visual = VisualStyle {
            background: Some(tokens.color(ColorToken::SliderTrack)),
            corner_radius: CornerRadius::uniform(FxPx::new(RADIUS)),
            inset_highlight: Some(tokens.color(ColorToken::SliderTrack)),
            opacity: self.state.is_disabled().then(|| self.state.opacity()),
            ..VisualStyle::default()
        };

        let glyph = self.leading.map(|node| alloc::vec![node]).unwrap_or_default();
        let fill = Self::bar(
            weight,
            Some(MIN_FILL),
            fill_visual,
            EdgeInsets { left: FxPx::new(GLYPH_INSET), ..EdgeInsets::zero() },
            glyph,
        );
        let mut children = alloc::vec![fill];
        // At 100 the rest would be a zero-weight, zero-width node: omit it
        // rather than ask the engine to lay out nothing.
        if weight < 100 {
            children.push(Self::bar(
                100 - weight,
                None,
                VisualStyle::default(),
                EdgeInsets::zero(),
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
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: Some(FxPx::new(TRACK_H)),
                max_height: Some(FxPx::new(TRACK_H)),
                // The track takes the row's free space (handoff `flex: 1`).
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
            },
            track_visual,
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    fn children(node: &LayoutNode) -> &[LayoutNode] {
        match node {
            LayoutNode::Stack(_, _, children) => children,
            _ => panic!("the slider builds a Stack"),
        }
    }

    fn stack(node: &LayoutNode) -> &Stack {
        match node {
            LayoutNode::Stack(stack, _, _) => stack,
            _ => panic!("expected a Stack"),
        }
    }

    #[test]
    fn fill_weight_tracks_value() {
        assert_eq!(Slider::new().value(0).fill_weight(), 0);
        assert_eq!(Slider::new().value(100).fill_weight(), 100);
        assert_eq!(Slider::new().value(200).value(50).fill_weight(), 50, "clamps");
    }

    /// The fill and the remainder always add up to the whole track, whatever
    /// width the row hands down. That is the property a pixel-computing
    /// builder could not have.
    #[test]
    fn fill_and_rest_weights_sum_to_the_whole_track() {
        let t = BaseTokens;
        for value in [0, 1, 37, 50, 99] {
            let node = Slider::new().value(value).build(&t);
            let kids = children(&node);
            assert_eq!(kids.len(), 2, "value {value}: fill + rest");
            let fill = stack(&kids[0]).item.flex_grow;
            let rest = stack(&kids[1]).item.flex_grow;
            assert_eq!(fill + rest, 100, "value {value}: weights must partition the track");
            assert_eq!(fill, u32::from(value));
        }
    }

    /// At zero the fill is still THERE — it is the handle. This inverts the
    /// old builder's contract, where value 0 dropped the fill bar entirely and
    /// left a bare thumb sitting on an empty track.
    #[test]
    fn the_fill_survives_value_zero_as_the_handle() {
        let t = BaseTokens;
        let node = Slider::new().value(0).build(&t);
        let kids = children(&node);
        let fill = stack(&kids[0]);
        assert_eq!(fill.item.flex_grow, 0, "no share of the free space…");
        assert_eq!(fill.min_width, Some(FxPx::new(MIN_FILL)), "…but never narrower than the cap");
    }

    #[test]
    fn full_value_has_no_rest_bar() {
        let t = BaseTokens;
        let node = Slider::new().value(100).build(&t);
        assert_eq!(children(&node).len(), 1, "the fill IS the track at 100");
    }

    /// The embedded glyph sits inside the FILL at a fixed inset, so the fill
    /// sweeps over it as the value grows instead of pushing it along.
    #[test]
    fn the_glyph_rides_inside_the_fill_at_a_fixed_inset() {
        let t = BaseTokens;
        let glyph = LayoutNode::Stack(
            Stack { id: Some("glyph"), ..Default::default() },
            VisualStyle::default(),
            alloc::vec![],
        );
        let node = Slider::new().value(70).leading(glyph).build(&t);
        let fill = &children(&node)[0];
        assert_eq!(stack(fill).padding.left, FxPx::new(GLYPH_INSET));
        assert_eq!(children(fill).len(), 1, "the glyph is a child of the fill, not the track");
        assert_eq!(stack(&children(fill)[0]).id, Some("glyph"));
    }

    /// A slider with no glyph is still a valid slider (the handoff's Ton panel
    /// reuses the same control without one in the compact case).
    #[test]
    fn the_glyph_is_optional() {
        let t = BaseTokens;
        let node = Slider::new().value(40).build(&t);
        assert!(children(&children(&node)[0]).is_empty());
    }

    #[test]
    fn the_id_reaches_the_track() {
        let t = BaseTokens;
        let node = Slider::new().value(60).id("vol").build(&t);
        assert_eq!(stack(&node).id, Some("vol"));
    }
}
