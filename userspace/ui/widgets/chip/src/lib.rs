// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Chip` — the design artifacts' chip (design `Chip`, RFC-0116): a capsule token for filters,
//! tags and recipients, one control height below its size — xs/sm 24, md 30 (default), lg 36,
//! xl 44 — with its label in medium weight (caption2 · caption · subheadline · body · headline).
//! Looks: neutral (`fill`, the label), tinted (the tint at 16 % with a tint label), filled (the
//! solid tint with its on-ink), outline (a separator hairline, or the tint at 55 %); a selected
//! chip is the solid tint with the soft glow under it. A leading icon or avatar (h − 6, 2 px from
//! the edge), a built-in remove button (an 18 px `fill` circle with a 9 px ×, by size), and a
//! .45 dim when disabled. Pure builder producing a capsule `LayoutNode::Stack`.

extern crate alloc;

#[cfg(test)]
mod tests;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, BoxShadow, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, FxPx,
    Justify, LayoutNode, Rgba8, Stack, VisualStyle,
};
use nexus_style::state::Dim;
use nexus_style::InteractionState;
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, TextStyleToken, Tokens, HAIRLINE};
use nexus_widget_icon::{with_ink, Icon, LucideSymbol};
use nexus_widget_text::Text;

/// The chip's look (design `Chip.appearance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChipAppearance {
    /// `fill`, the label (the tint's label when a tint is set).
    #[default]
    Neutral,
    /// The tint at 16 % with a tint label.
    Tinted,
    /// The solid tint with its on-ink.
    Filled,
    /// Transparent with a hairline (`separator`, or the tint at 55 %).
    Outline,
}

/// One size's metrics: height, label style, horizontal padding, gap, remove button and glyph.
struct Metrics {
    h: i32,
    text: TextStyleToken,
    pad_x: i32,
    gap: i32,
    x: i32,
    xi: i32,
}

const fn metrics(size: ControlSize) -> Metrics {
    match size {
        ControlSize::Xs => {
            Metrics { h: 24, text: TextStyleToken::Caption2, pad_x: 8, gap: 4, x: 14, xi: 7 }
        }
        ControlSize::Sm => {
            Metrics { h: 24, text: TextStyleToken::Caption, pad_x: 8, gap: 4, x: 16, xi: 8 }
        }
        ControlSize::Md => {
            Metrics { h: 30, text: TextStyleToken::Subheadline, pad_x: 12, gap: 6, x: 18, xi: 9 }
        }
        ControlSize::Lg => {
            Metrics { h: 36, text: TextStyleToken::Body, pad_x: 16, gap: 6, x: 20, xi: 10 }
        }
        ControlSize::Xl => {
            Metrics { h: 44, text: TextStyleToken::Headline, pad_x: 16, gap: 8, x: 22, xi: 11 }
        }
    }
}

/// `color` at `pct` % of its own alpha (the design's `color-mix(… pct %, transparent)`).
fn wash(color: Rgba8, pct: u32) -> Rgba8 {
    Rgba8 { a: (u32::from(color.a) * pct / 100) as u8, ..color }
}

/// The ink that reads on a solid `tint`.
fn on_tint(tint: ColorToken) -> ColorToken {
    match tint {
        ColorToken::Primary | ColorToken::Label => ColorToken::OnPrimary,
        _ => ColorToken::OnAccent,
    }
}

/// A selectable / removable chip.
#[derive(Debug, Clone, Default)]
pub struct Chip {
    label: String,
    selected: bool,
    leading: Option<LayoutNode>,
    avatar: Option<LayoutNode>,
    remove: Option<&'static str>,
    size: ControlSize,
    appearance: ChipAppearance,
    tint: Option<ColorToken>,
    state: InteractionState,
    id: Option<&'static str>,
}

impl Chip {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), ..Self::default() }
    }

    /// The selected look: the solid tint with the glow under it.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    /// A leading icon; the chip inks it with its label.
    pub fn leading(mut self, leading: LayoutNode) -> Self {
        self.leading = Some(leading);
        self
    }
    /// A leading avatar, built at [`Self::avatar_size`] (h − 6); the chip pads 2 px before it.
    pub fn avatar(mut self, avatar: LayoutNode) -> Self {
        self.avatar = Some(avatar);
        self
    }
    /// The built-in remove button, with `id` as its tap target.
    pub fn removable(mut self, id: &'static str) -> Self {
        self.remove = Some(id);
        self
    }
    /// The control size (default `Md`: a 30 px chip).
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
    pub fn appearance(mut self, appearance: ChipAppearance) -> Self {
        self.appearance = appearance;
        self
    }
    /// The tint (default `accent`); a neutral or outline chip colours its label with it.
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
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

    /// The chip's outer height (24 · 24 · 30 · 36 · 44).
    #[must_use]
    pub fn height(&self) -> i32 {
        metrics(self.size).h
    }

    /// The size a leading avatar is built at: concentric, 2 px inside the 1 px edge.
    #[must_use]
    pub fn avatar_size(&self) -> i32 {
        self.height() - 6
    }

    fn tint_role(&self) -> ColorToken {
        self.tint.unwrap_or(ColorToken::Accent)
    }

    /// Whether the chip is a solid tint (selected or filled).
    fn on_fill(&self) -> bool {
        self.selected || self.appearance == ChipAppearance::Filled
    }

    /// The label ink's role for the current look.
    pub fn foreground(&self) -> ColorToken {
        if self.on_fill() {
            on_tint(self.tint_role())
        } else if self.appearance == ChipAppearance::Tinted {
            self.tint_role()
        } else {
            self.tint.unwrap_or(ColorToken::Label)
        }
    }

    fn surface(&self, tokens: &dyn Tokens) -> VisualStyle {
        let tint = tokens.color(self.tint_role());
        let mut visual = VisualStyle {
            corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
            ..VisualStyle::default()
        };
        if self.on_fill() {
            visual.background = Some(tint);
            if self.selected {
                let glow = match self.tint_role() {
                    ColorToken::Accent => tokens.color(ColorToken::AccentGlow),
                    _ => wash(tint, 38),
                };
                visual.shadow = Some(BoxShadow {
                    offset_x: FxPx::ZERO,
                    offset_y: FxPx::new(2),
                    blur_radius: FxPx::new(8),
                    spread: FxPx::ZERO,
                    color: glow,
                });
            }
        } else {
            match self.appearance {
                ChipAppearance::Tinted => visual.background = Some(wash(tint, 16)),
                ChipAppearance::Outline => {
                    let line = if self.tint.is_some() {
                        wash(tint, 55)
                    } else {
                        tokens.color(ColorToken::Separator)
                    };
                    visual.border = EdgeBorder::all(HAIRLINE, line);
                }
                _ => visual.background = Some(tokens.color(ColorToken::Fill)),
            }
        }
        if self.state.is_disabled() {
            visual.opacity = Some(self.state.opacity_as(Dim::Control));
        }
        visual
    }

    /// The remove button: an `x`-px circle (`fill`, or the on-ink at 30 % on a solid chip) with
    /// an `xi`-px × (stroke 3) in the label's ink.
    fn remove_button(&self, tokens: &dyn Tokens, id: &'static str, m: &Metrics) -> LayoutNode {
        let (bg, ink) = if self.on_fill() {
            let on = tokens.color(on_tint(self.tint_role()));
            (wash(on, 30), on)
        } else {
            (tokens.color(ColorToken::Fill), tokens.color(ColorToken::Label))
        };
        let glyph = with_ink(
            Icon::lucide(LucideSymbol::Xmark).stroke_tenths(30).size(m.xi).build(tokens),
            ink,
        );
        let d = Some(FxPx::new(m.x));
        LayoutNode::Stack(
            Stack {
                id: Some(id),
                direction: Direction::Row,
                align: Align::Center,
                justify: Justify::Center,
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle {
                background: Some(bg),
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            alloc::vec![glyph],
        )
    }

    /// Build the chip node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let m = metrics(self.size);
        let visual = self.surface(tokens);
        let ink = tokens.color(self.foreground());

        let mut children: Vec<LayoutNode> = Vec::new();
        if let Some(avatar) = self.avatar.clone() {
            children.push(avatar);
        }
        if let Some(icon) = self.leading.clone() {
            children.push(with_ink(icon, ink));
        }
        let mut label = Text::new(self.label.clone())
            .text_style(m.text)
            .weight(FontWeight::Medium)
            .build(tokens);
        if let LayoutNode::Text(t, _) = &mut label {
            t.style.color = ink;
        }
        children.push(label);
        if let Some(id) = self.remove {
            children.push(self.remove_button(tokens, id, &m));
        }

        // The design's 1 px edge (transparent unless outlined) sits outside the padding; the
        // painter draws borders inside the box, so the padding carries that pixel.
        let pad_l = if self.avatar.is_some() { 2 } else { m.pad_x } + 1;
        let pad_r = if self.remove.is_some() { 4 } else { m.pad_x } + 1;
        let h = Some(FxPx::new(m.h));
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(m.gap),
                padding: EdgeInsets {
                    top: FxPx::ZERO,
                    right: FxPx::new(pad_r),
                    bottom: FxPx::ZERO,
                    left: FxPx::new(pad_l),
                },
                align: Align::Center,
                justify: Justify::Center,
                min_height: h,
                max_height: h,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            visual,
            children,
        )
    }
}
