// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `AppIcon` — the design-system app-icon tile (handoff `AppIcon`): three
//! rendering variants (`native` shaped tile · `wrapped` glass backing ·
//! `freestanding` bare) × five sizes, wrapping a caller-provided icon node.
//! A pure builder producing a `LayoutNode::Stack` tile. The notification `badge`
//! and `active` dot are exposed as data — they render **outside** the tile
//! (never clipped) and are placed by the compositor as overlays, so they are not
//! part of the clipped tile subtree. DSL-emittable.

extern crate alloc;

use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};

/// Translucency of the wrapped variant's glass backing (0..255).
const GLASS_FILL_ALPHA: u8 = 38; // ~0.15, the handoff --glass-icon-bg
const GLASS_BLUR_RADIUS: u32 = 12;
const GLASS_SATURATION: u32 = 140;

/// Rendering variant (handoff `AppIconProps.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppIconVariant {
    /// Icon fills the rounded tile directly (first-party, shaped icons).
    #[default]
    Native,
    /// Glass panel backing wraps the icon (sideloaded/web/shapeless icons).
    Wrapped,
    /// Bare, no backing (pre-composed special-folder icons).
    Freestanding,
}

/// Size preset (handoff `AppIconProps.size`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppIconSize {
    Xs,
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
}

impl AppIconSize {
    /// Tile edge length in logical px.
    pub fn dimension(self) -> i32 {
        match self {
            AppIconSize::Xs => 32,
            AppIconSize::Sm => 40,
            AppIconSize::Md => 56,
            AppIconSize::Lg => 72,
            AppIconSize::Xl => 96,
        }
    }

    /// Corner radius token (squircle-ish per size).
    pub fn radius(self) -> LengthToken {
        match self {
            AppIconSize::Xs | AppIconSize::Sm => LengthToken::RadiusSmall,
            AppIconSize::Md => LengthToken::RadiusMedium,
            AppIconSize::Lg | AppIconSize::Xl => LengthToken::RadiusLarge,
        }
    }

    /// Inner inset for the `wrapped` variant (icon ~65% of the tile).
    fn wrapped_inset(self) -> FxPx {
        FxPx::new((self.dimension() * 175 / 1000).max(4))
    }
}

/// An app-icon tile.
#[derive(Debug, Clone, Default)]
pub struct AppIcon {
    variant: AppIconVariant,
    size: AppIconSize,
    badge: Option<u32>,
    active: bool,
    id: Option<&'static str>,
    content: Option<LayoutNode>,
}

impl AppIcon {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn variant(mut self, variant: AppIconVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: AppIconSize) -> Self {
        self.size = size;
        self
    }

    /// Notification count badge (rendered outside the tile by the compositor;
    /// `None` or `0` = hidden).
    pub fn badge(mut self, count: u32) -> Self {
        self.badge = Some(count);
        self
    }

    /// Active-app indicator dot (dock).
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The icon node (caller-provided image/SVG).
    pub fn content(mut self, content: LayoutNode) -> Self {
        self.content = Some(content);
        self
    }

    /// The badge count to render as an overlay (`None` when 0/unset).
    pub fn badge_count(&self) -> Option<u32> {
        self.badge.filter(|&n| n > 0)
    }

    /// Whether to render the active-app dot overlay.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Resolved tile [`Style`] (backing depends on the variant).
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let mut s = Style::new().rounded(tokens.length(self.size.radius()));
        if matches!(self.variant, AppIconVariant::Wrapped) {
            let mut bg = tokens.color(ColorToken::Surface);
            bg.a = GLASS_FILL_ALPHA;
            s = s
                .background(bg)
                .border_token(tokens, LengthToken::BorderThin, ColorToken::Border)
                .blur(GLASS_BLUR_RADIUS, GLASS_SATURATION);
        }
        s
    }

    /// Build the tile node (wraps the icon content).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let dim = FxPx::new(self.size.dimension());
        // `native` clips the icon to the rounded tile; `wrapped` insets it.
        let (padding, overflow) = match self.variant {
            AppIconVariant::Native => (EdgeInsets::zero(), Overflow::Hidden),
            AppIconVariant::Wrapped => (EdgeInsets::all(self.size.wrapped_inset()), Overflow::Hidden),
            AppIconVariant::Freestanding => (EdgeInsets::zero(), Overflow::Visible),
        };
        let visual = self.style(tokens).visual();
        let children = match self.content {
            Some(c) => alloc::vec![c],
            None => alloc::vec![],
        };
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding,
                align: Align::Center,
                justify: Justify::Center,
                overflow,
                flex_wrap: false,
                min_width: Some(dim),
                max_width: Some(dim),
                min_height: Some(dim),
                max_height: Some(dim),
                item: FlexItem::default(),
            },
            visual,
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn wrapped_has_glass_backing_native_does_not() {
        let t = BaseTokens;
        let wrapped = AppIcon::new().variant(AppIconVariant::Wrapped).style(&t);
        assert!(wrapped.visual().background.is_some());
        assert!(wrapped.backdrop_blur().is_some());
        let native = AppIcon::new().variant(AppIconVariant::Native).style(&t);
        assert!(native.visual().background.is_none());
    }

    #[test]
    fn sizes_drive_tile_dimension() {
        let t = BaseTokens;
        let dim = |s: AppIconSize| match AppIcon::new().size(s).build(&t) {
            LayoutNode::Stack(stack, _, _) => stack.min_width,
            _ => panic!(),
        };
        assert_eq!(dim(AppIconSize::Xs), Some(FxPx::new(32)));
        assert_eq!(dim(AppIconSize::Xl), Some(FxPx::new(96)));
    }

    #[test]
    fn badge_and_active_are_overlay_data() {
        assert_eq!(AppIcon::new().badge(3).badge_count(), Some(3));
        assert_eq!(AppIcon::new().badge(0).badge_count(), None); // hidden at 0
        assert!(AppIcon::new().active(true).is_active());
    }

    #[test]
    fn native_clips_freestanding_does_not() {
        let t = BaseTokens;
        let overflow = |v: AppIconVariant| match AppIcon::new().variant(v).build(&t) {
            LayoutNode::Stack(stack, _, _) => stack.overflow,
            _ => panic!(),
        };
        assert_eq!(overflow(AppIconVariant::Native), Overflow::Hidden);
        assert_eq!(overflow(AppIconVariant::Freestanding), Overflow::Visible);
    }
}
