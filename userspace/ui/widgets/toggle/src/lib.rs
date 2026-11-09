// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `GlassToggle` — the design-system on/off control (the design artifacts' `GlassToggle`).
//!
//! The switch (default): a capsule track in `fill`, the tint and its small glow when on, and a
//! white thumb (`onAccent`, `shadow1`) — five sizes from 32 × 19 to 58 × 35, the thumb inset 3
//! (2 below a 26 px track). An optional visible label sits left of the track (gap 12, control
//! label); the DSL keeps its `label:` as the accessible name, so it does not route here.
//!
//! The button appearance (the artifacts' toggle button): a capsule of the control height in
//! `fill` with the glass edge, the tint fill and its medium glow when on, its label in the
//! on-colour; a circle when it has an icon and no label.
//!
//! A pure builder producing a `LayoutNode::Stack`. Hover and press never repaint it (the host's
//! layer and its thumb stretch); the state draws the disabled dim (0.45) and the focus ring.
//! Tree of the bare switch: root = the track (the interaction id), child 0 = the thumb, placed
//! by `justify` — the host's press stretches and slides that child (the DSL's `press_offset`).

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Rgba8, Stack, TextContent,
    TextNode, VisualStyle,
};
use nexus_style::control::{glow_shadow, label_style, metrics, on_tint, Glow};
use nexus_style::{Dim, InteractionState, Style};
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, ShadowToken, Tokens};

/// The switch geometry of a size (px).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchMetrics {
    pub width: FxPx,
    pub height: FxPx,
    /// The thumb's inset from the track edge.
    pub inset: FxPx,
    /// The thumb diameter.
    pub thumb: FxPx,
    /// How far the thumb travels from off to on.
    pub travel: FxPx,
    /// How much wider the thumb grows while pressed (round(thumb × 0.3)), anchored to its side.
    pub grow: FxPx,
}

impl SwitchMetrics {
    /// The switch of a size: xs 32 × 19 · sm 38 × 22 · md 44 × 26 · lg 51 × 31 · xl 58 × 35.
    #[must_use]
    pub fn of(size: ControlSize) -> Self {
        let (w, h) = match size {
            ControlSize::Xs => (32, 19),
            ControlSize::Sm => (38, 22),
            ControlSize::Md => (44, 26),
            ControlSize::Lg => (51, 31),
            ControlSize::Xl => (58, 35),
        };
        let inset = if h >= 26 { 3 } else { 2 };
        let thumb = h - 2 * inset;
        SwitchMetrics {
            width: FxPx::new(w),
            height: FxPx::new(h),
            inset: FxPx::new(inset),
            thumb: FxPx::new(thumb),
            travel: FxPx::new(w - thumb - 2 * inset),
            grow: FxPx::new((thumb * 3 + 5) / 10),
        }
    }

    /// The pressed thumb's horizontal stretch in thousandths ((thumb + grow) / thumb).
    #[must_use]
    pub fn stretch_milli(self) -> u32 {
        let thumb = self.thumb.0.max(1) as u32;
        (thumb + self.grow.0.max(0) as u32) * 1000 / thumb
    }
}

/// How the toggle is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToggleAppearance {
    /// The switch: track + thumb.
    #[default]
    Switch,
    /// A selectable capsule (a circle when icon-only) whose tint fill shows when on.
    Button,
}

/// The on/off control.
#[derive(Debug, Clone, Default)]
pub struct GlassToggle {
    checked: bool,
    state: InteractionState,
    id: Option<&'static str>,
    size: ControlSize,
    tint: Option<ColorToken>,
    appearance: ToggleAppearance,
    label: Option<String>,
    icon: Option<LayoutNode>,
}

impl GlassToggle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
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

    /// The control size (the switch track 32…58 px wide).
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// The on-state's colour role (default accent; e.g. `modeAirplane`).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }

    pub fn appearance(mut self, appearance: ToggleAppearance) -> Self {
        self.appearance = appearance;
        self
    }

    /// A visible label: left of the switch, inside the button appearance.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The button appearance's icon (built by the caller in [`Self::foreground_role`]).
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }

    fn tint_role(&self) -> ColorToken {
        self.tint.unwrap_or(ColorToken::Accent)
    }

    /// The track fill for the current checked state (the tint when on, `fill` off).
    pub fn track_color(&self, tokens: &dyn Tokens) -> Rgba8 {
        tokens.color(if self.checked { self.tint_role() } else { ColorToken::Fill })
    }

    /// The label colour role of the button appearance: the on-colour when on, the tint (when
    /// one is set) or `label` when off.
    pub fn foreground_role(&self) -> ColorToken {
        match (self.checked, self.tint) {
            (true, _) => on_tint(self.tint_role()),
            (false, Some(tint)) => tint,
            (false, None) => ColorToken::Label,
        }
    }

    /// Focus ring and disabled dim on the root.
    fn finish(&self, style: Style, tokens: &dyn Tokens) -> Style {
        let mut s = style;
        if self.state.shows_focus_ring() {
            s = s.focus_ring(tokens, ColorToken::Accent);
        }
        if self.state.is_disabled() {
            s = s.opacity(self.state.opacity_as(Dim::Control));
        }
        s
    }

    fn text(text: String, style: nexus_layout_types::TextStyle, grow: bool) -> LayoutNode {
        LayoutNode::Text(
            TextNode {
                content: TextContent::new(text),
                style,
                max_lines: Some(1),
                item: FlexItem { flex_grow: u32::from(grow), ..FlexItem::default() },
                ..TextNode::default()
            },
            VisualStyle::default(),
        )
    }

    fn thumb(m: SwitchMetrics, tokens: &dyn Tokens) -> LayoutNode {
        // White in both themes (the artifacts' knob), lifted by `shadow1`.
        let visual = Style::new()
            .background(tokens.color(ColorToken::OnAccent))
            .rounded_token(RadiusToken::Full)
            .shadow_token(tokens, ShadowToken::Shadow1)
            .visual();
        let d = Some(m.thumb);
        LayoutNode::Stack(
            Stack {
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            visual,
            Vec::new(),
        )
    }

    fn track(&self, tokens: &dyn Tokens, root: bool) -> LayoutNode {
        let m = SwitchMetrics::of(self.size);
        let mut style =
            Style::new().background(self.track_color(tokens)).rounded_token(RadiusToken::Full);
        if self.checked {
            style = style.shadow(glow_shadow(tokens, self.tint_role(), Glow::Sm));
        }
        if root {
            style = self.finish(style, tokens);
        }
        LayoutNode::Stack(
            Stack {
                id: if root { self.id } else { None },
                direction: Direction::Row,
                padding: EdgeInsets::all(m.inset),
                align: Align::Center,
                // The thumb sits at the trailing end when on.
                justify: if self.checked { Justify::End } else { Justify::Start },
                min_width: Some(m.width),
                max_width: Some(m.width),
                min_height: Some(m.height),
                max_height: Some(m.height),
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            style.visual(),
            alloc::vec![Self::thumb(m, tokens)],
        )
    }

    fn switch(self, tokens: &dyn Tokens) -> LayoutNode {
        let Some(label) = self.label.clone() else {
            return self.track(tokens, true);
        };
        let ink = tokens.color(ColorToken::Label);
        let style = self.finish(Style::new().rounded_token(RadiusToken::Sm), tokens);
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(12),
                align: Align::Center,
                ..Stack::default()
            },
            style.visual(),
            alloc::vec![
                Self::text(label, label_style(self.size, ink), true),
                self.track(tokens, false)
            ],
        )
    }

    fn button(self, tokens: &dyn Tokens) -> LayoutNode {
        let m = metrics(self.size);
        let circle = self.icon.is_some() && self.label.as_deref().is_none_or(str::is_empty);
        let mut style = Style::new().rounded_token(RadiusToken::Full);
        style = if self.checked {
            // The tint layer covers the edge.
            style.background(tokens.color(self.tint_role())).shadow(glow_shadow(
                tokens,
                self.tint_role(),
                Glow::Md,
            ))
        } else {
            style
                .background(tokens.color(ColorToken::Fill))
                .border_token(tokens, ColorToken::GlassEdge)
        };
        let style = self.finish(style, tokens);
        let ink = tokens.color(self.foreground_role());
        let mut items: Vec<LayoutNode> = self.icon.clone().into_iter().collect();
        if !circle {
            if let Some(label) = self.label.clone() {
                items.push(Self::text(label, label_style(self.size, ink), false));
            }
        }
        let h = Some(m.height);
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: m.gap,
                padding: if circle {
                    EdgeInsets::zero()
                } else {
                    EdgeInsets::symmetric(FxPx::ZERO, m.pad_x)
                },
                align: Align::Center,
                justify: Justify::Center,
                min_width: if circle { h } else { None },
                max_width: if circle { h } else { None },
                min_height: h,
                max_height: h,
                ..Stack::default()
            },
            style.visual(),
            items,
        )
    }

    /// Build the toggle node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        match self.appearance {
            ToggleAppearance::Switch => self.switch(tokens),
            ToggleAppearance::Button => self.button(tokens),
        }
    }
}

#[cfg(test)]
mod tests;
