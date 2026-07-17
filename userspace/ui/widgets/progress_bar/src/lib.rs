// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `ProgressBar` — the design-system progress track (handoff `ProgressBar`):
//! a glass track with an accent-blue fill. Determinate (`value` 0–100) or
//! indeterminate (a sliding pip whose position is the motion `phase` —
//! the motion system advances it; a static build renders one frame).
//! A pure `LayoutNode` builder from theme tokens. DSL-emittable.

extern crate alloc;

use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    Position, Rgba8, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, Tokens};

/// Track translucency (the glass groove under the fill).
const TRACK_ALPHA: u8 = 56;
/// Indeterminate pip width as a fraction of the track (percent).
const PIP_PERCENT: u32 = 28;

/// A progress track.
#[derive(Debug, Clone)]
pub struct ProgressBar {
    /// 0–100; ignored when `indeterminate`.
    value: u32,
    indeterminate: bool,
    /// Pip phase 0–100 (leading-edge position) while indeterminate.
    phase: u32,
    width: FxPx,
    height: FxPx,
    color: ColorToken,
    id: Option<&'static str>,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self {
            value: 0,
            indeterminate: false,
            phase: 0,
            width: FxPx::new(200),
            height: FxPx::new(6),
            color: ColorToken::Info,
            id: None,
        }
    }
}

impl ProgressBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Determinate progress, clamped to 0–100 (handoff `value`).
    pub fn value(mut self, percent: u32) -> Self {
        self.value = percent.min(100);
        self.indeterminate = false;
        self
    }

    /// Indeterminate mode: a sliding pip instead of a fixed fill.
    pub fn indeterminate(mut self) -> Self {
        self.indeterminate = true;
        self
    }

    /// Pip position 0–100 while indeterminate (motion-system driven).
    pub fn phase(mut self, phase: u32) -> Self {
        self.phase = phase.min(100);
        self
    }

    /// Track width in px (the DSL layout may stretch the node further).
    pub fn width(mut self, px: i32) -> Self {
        self.width = FxPx::new(px.max(8));
        self
    }

    /// Track height in px (handoff `height`; default 6).
    pub fn height(mut self, px: i32) -> Self {
        self.height = FxPx::new(px.max(2));
        self
    }

    /// Fill color token (defaults to `Info` — the handoff blue).
    pub fn color(mut self, color: ColorToken) -> Self {
        self.color = color;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// `(fill_left_px, fill_width_px)` inside the track for the current state.
    fn fill_geometry(&self) -> (i32, i32) {
        let track = self.width.0;
        if self.indeterminate {
            let pip = track * PIP_PERCENT as i32 / 100;
            let travel = track - pip;
            (travel * self.phase as i32 / 100, pip)
        } else {
            (0, track * self.value as i32 / 100)
        }
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let radius = CornerRadius::uniform(FxPx::new(self.height.0 / 2));
        let track_base = tokens.color(ColorToken::OnSurface);
        let track = Rgba8::new(track_base.r, track_base.g, track_base.b, TRACK_ALPHA);
        let fill = tokens.color(self.color);
        let (left, fill_w) = self.fill_geometry();
        let h = Some(self.height);
        let mut children = alloc::vec::Vec::new();
        if fill_w > 0 {
            children.push(LayoutNode::Stack(
                Stack {
                    id: None,
                    direction: Direction::Row,
                    gap: FxPx::ZERO,
                    padding: EdgeInsets::zero(),
                    align: Align::Center,
                    justify: Justify::Start,
                    overflow: Overflow::Visible,
                    flex_wrap: false,
                    min_width: Some(FxPx::new(fill_w)),
                    max_width: Some(FxPx::new(fill_w)),
                    min_height: h,
                    max_height: h,
                    item: FlexItem {
                        position: Position::Absolute,
                        margin: EdgeInsets { left: FxPx::new(left), ..EdgeInsets::zero() },
                        ..FlexItem::default()
                    },
                },
                VisualStyle {
                    background: Some(fill),
                    corner_radius: radius.clone(),
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
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: Some(self.width),
                max_width: Some(self.width),
                min_height: h,
                max_height: h,
                item: FlexItem::default(),
            },
            VisualStyle {
                background: Some(track),
                corner_radius: radius,
                ..VisualStyle::default()
            },
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn determinate_fill_scales_with_value() {
        let (l0, w0) = ProgressBar::new().width(200).value(0).fill_geometry();
        let (l50, w50) = ProgressBar::new().width(200).value(50).fill_geometry();
        let (l100, w100) = ProgressBar::new().width(200).value(100).fill_geometry();
        assert_eq!((l0, w0), (0, 0));
        assert_eq!((l50, w50), (0, 100));
        assert_eq!((l100, w100), (0, 200));
    }

    #[test]
    fn indeterminate_pip_travels_the_track() {
        let (start, pip) = ProgressBar::new().width(200).indeterminate().phase(0).fill_geometry();
        let (end, pip2) = ProgressBar::new().width(200).indeterminate().phase(100).fill_geometry();
        assert_eq!(pip, pip2);
        assert_eq!(start, 0);
        assert_eq!(end + pip, 200, "at phase 100 the pip touches the right edge");
    }

    #[test]
    fn value_clamps_and_builds() {
        let node = ProgressBar::new().value(250).build(&BaseTokens);
        match node {
            LayoutNode::Stack(_, _, children) => assert_eq!(children.len(), 1),
            _ => panic!("track must be a stack"),
        }
    }
}
