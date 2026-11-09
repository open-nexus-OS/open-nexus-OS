// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Slider` — the design-system range slider (the design artifacts' `Slider`), value 0..=100.
//!
//! [`SliderVariant::Control`] (the builder's default, the Control Center bar): a 28 px
//! capsule in `fill` with the recessed well, and a white fill (`onAccent`, `shadow1`; an
//! explicit tint colours it) growing from the left that never gets narrower than its 28 px
//! cap — its leading edge IS the handle. The glyph rides inside the fill at a fixed 7 px inset
//! in `island` at 50 % (the tint's on-colour at 85 % on a tinted fill); the fill sweeps over it.
//!
//! [`SliderVariant::Thumb`] (the artifacts' default slider): a 28 px hit track holding a 6 px
//! `fill` track, the tint fill with its glow up to the value, a 20 px white thumb (`shadow1`)
//! and optional tick marks below the track.
//!
//! Width is NOT fixed: the root takes the row's free space, and the fill/rest split is expressed
//! as flex WEIGHTS (value vs 100 − value) — the layout equivalent of the design's `width: 70%`,
//! so the builder never needs the row width. The root is the track in both variants: a tap's
//! fraction across the root IS the value (the DSL's track-fraction bind). The glyph arrives as a
//! ready-made node (`leading`), so this crate stays free of the icon set. Hover never repaints;
//! a disabled slider dims to 0.45.

extern crate alloc;

mod thumb;

use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Stack, VisualStyle,
};
use nexus_style::control::{on_tint, with_alpha_percent};
use nexus_style::{Dim, InteractionState, Style};
use nexus_theme_tokens::{ColorToken, RadiusToken, ShadowToken, Tokens};

/// Track height of the control bar (and the thumb variant's hit track); a true capsule.
const TRACK_H: i32 = 28;
/// The fill never shrinks below this: its leading edge IS the handle, and a
/// handle you cannot see is one you cannot grab.
const MIN_FILL: i32 = 28;
/// Inset of the embedded glyph from the track's leading edge. Fixed — the
/// glyph does not travel with the value, the fill sweeps OVER it.
const GLYPH_INSET: i32 = 7;

/// Which slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SliderVariant {
    /// The Control Center bar: a white fill carrying the glyph, no thumb.
    #[default]
    Control,
    /// The artifacts' default slider: a thin track, a tint fill and a white thumb.
    Thumb,
}

/// A horizontal range slider (value 0..=100).
#[derive(Debug, Default)]
pub struct Slider {
    value: u8,
    variant: SliderVariant,
    state: InteractionState,
    id: Option<&'static str>,
    leading: Option<LayoutNode>,
    tint: Option<ColorToken>,
    ticks: u8,
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

    pub fn variant(mut self, variant: SliderVariant) -> Self {
        self.variant = variant;
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

    /// The glyph embedded in the control bar's fill (brightness sun, volume speaker). Built by
    /// the caller so this crate needs no icon dependency; the slider sets its colour.
    pub fn leading(mut self, node: LayoutNode) -> Self {
        self.leading = Some(node);
        self
    }

    /// An explicit tint: the thumb slider's fill (default accent); on the control bar it
    /// colours the fill (default white).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }

    /// Tick marks below the thumb slider's track: `count` evenly spaced, ends included (0 = none).
    pub fn ticks(mut self, count: u8) -> Self {
        self.ticks = count;
        self
    }

    /// The fill's share of the track, as a flex weight. The remainder gets
    /// `100 - share`, so the two always divide the row exactly.
    pub fn fill_weight(&self) -> u32 {
        self.value as u32
    }

    fn dim(&self, visual: &mut VisualStyle) {
        if self.state.is_disabled() {
            visual.opacity = Some(self.state.opacity_as(Dim::Control));
        }
    }

    /// The root track node (both variants): it takes the row's free space.
    fn track(&self, visual: VisualStyle, children: alloc::vec::Vec<LayoutNode>) -> LayoutNode {
        let h = Some(FxPx::new(TRACK_H));
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                align: Align::Center,
                justify: Justify::Start,
                min_height: h,
                max_height: h,
                // The track takes the row's free space (design `flex: 1`).
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
                ..Stack::default()
            },
            visual,
            children,
        )
    }

    /// The glyph in the control bar's fill: `island` at 50 %, or the tint's on-colour at 85 %
    /// on a tinted fill. The row painter has no group opacity, so the emphasis is the glyph's
    /// paint (a glyph is one shaped box).
    fn glyph(&self, node: LayoutNode, tokens: &dyn Tokens) -> LayoutNode {
        let color = match self.tint {
            Some(tint) => with_alpha_percent(tokens.color(on_tint(tint)), 85),
            None => with_alpha_percent(tokens.color(ColorToken::Island), 50),
        };
        match node {
            LayoutNode::Stack(stack, mut visual, children) => {
                if visual.background.is_some() {
                    visual.background = Some(color);
                }
                LayoutNode::Stack(stack, visual, children)
            }
            other => other,
        }
    }

    fn control(mut self, tokens: &dyn Tokens) -> LayoutNode {
        let weight = self.fill_weight();
        let fill_visual = Style::new()
            .background(tokens.color(self.tint.unwrap_or(ColorToken::OnAccent)))
            .rounded_token(RadiusToken::Full)
            .shadow_token(tokens, ShadowToken::Shadow1)
            .visual();
        // The recessed well (`shadowInset`) as the painter's one-pixel top line in its dark
        // colour — the painter has no blurred inset shadow.
        let mut track_visual = Style::new()
            .background_token(tokens, ColorToken::Fill)
            .rounded_token(RadiusToken::Full)
            .shadow_token(tokens, ShadowToken::Inset)
            .visual();
        self.dim(&mut track_visual);
        let glyph =
            self.leading.take().map(|n| alloc::vec![self.glyph(n, tokens)]).unwrap_or_default();
        let fill = bar(
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
            children.push(bar(
                100 - weight,
                None,
                VisualStyle::default(),
                EdgeInsets::zero(),
                alloc::vec![],
            ));
        }
        self.track(track_visual, children)
    }

    /// Build the slider node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        match self.variant {
            SliderVariant::Control => self.control(tokens),
            SliderVariant::Thumb => self.thumb(tokens),
        }
    }
}

/// A flex bar: `weight` decides its share of the track, `min_w` its floor.
fn bar(
    weight: u32,
    min_w: Option<i32>,
    visual: VisualStyle,
    padding: EdgeInsets,
    children: alloc::vec::Vec<LayoutNode>,
) -> LayoutNode {
    let h = Some(FxPx::new(TRACK_H));
    LayoutNode::Stack(
        Stack {
            direction: Direction::Row,
            padding,
            align: Align::Center,
            justify: Justify::Start,
            min_width: min_w.map(FxPx::new),
            min_height: h,
            max_height: h,
            item: FlexItem { flex_grow: weight, ..FlexItem::default() },
            ..Stack::default()
        },
        visual,
        children,
    )
}

#[cfg(test)]
mod tests;
