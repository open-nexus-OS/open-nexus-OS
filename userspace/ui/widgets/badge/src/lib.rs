// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Badge` — the design-system status chip (handoff `Badge`): 8 variants,
//! resolved from theme tokens. A pure builder producing a small pill
//! `LayoutNode::Stack` around caller-provided content (text), with the resolved
//! foreground exposed so the caller colors the label. DSL-emittable.

extern crate alloc;

use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Rgba8, Stack,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};

/// Translucency of the glass variant's fill (0..255).
const GLASS_FILL_ALPHA: u8 = 170;
const GLASS_BLUR_RADIUS: u32 = 12;
const GLASS_SATURATION: u32 = 140;

/// Visual variant (handoff `BadgeProps.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeVariant {
    #[default]
    Default,
    Secondary,
    Glass,
    Destructive,
    Success,
    Warning,
    Outline,
    Active,
}

/// A status chip.
#[derive(Debug, Clone, Default)]
pub struct Badge {
    variant: BadgeVariant,
    id: Option<&'static str>,
    content: Option<LayoutNode>,
}

impl Badge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    pub fn content(mut self, content: LayoutNode) -> Self {
        self.content = Some(content);
        self
    }

    /// The resolved foreground (label) color for this variant.
    pub fn foreground(&self, tokens: &dyn Tokens) -> Rgba8 {
        tokens.color(match self.variant {
            BadgeVariant::Destructive => ColorToken::OnDanger,
            BadgeVariant::Success => ColorToken::OnSuccess,
            BadgeVariant::Warning => ColorToken::OnWarning,
            BadgeVariant::Active => ColorToken::OnAccent,
            BadgeVariant::Secondary => ColorToken::OnSurfaceVariant,
            BadgeVariant::Default | BadgeVariant::Glass | BadgeVariant::Outline => {
                ColorToken::OnSurface
            }
        })
    }

    /// Container background (None = transparent, for `Outline`).
    fn background(&self, tokens: &dyn Tokens) -> Option<Rgba8> {
        match self.variant {
            BadgeVariant::Outline => None,
            BadgeVariant::Default => Some(tokens.color(ColorToken::SurfaceVariant)),
            BadgeVariant::Secondary => Some(tokens.color(ColorToken::Surface)),
            BadgeVariant::Destructive => Some(tokens.color(ColorToken::Danger)),
            BadgeVariant::Success => Some(tokens.color(ColorToken::Success)),
            BadgeVariant::Warning => Some(tokens.color(ColorToken::Warning)),
            BadgeVariant::Active => Some(tokens.color(ColorToken::Accent)),
            BadgeVariant::Glass => {
                let mut c = tokens.color(ColorToken::Surface);
                c.a = GLASS_FILL_ALPHA;
                Some(c)
            }
        }
    }

    /// The resolved container [`Style`].
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let mut s = Style::new();
        if let Some(bg) = self.background(tokens) {
            s = s.background(bg);
        }
        // Pill: medium radius reads as fully rounded at chip height.
        s = s.rounded(tokens.length(LengthToken::RadiusMedium));
        if matches!(self.variant, BadgeVariant::Outline | BadgeVariant::Glass) {
            s = s.border_token(tokens, LengthToken::BorderThin, ColorToken::Border);
        }
        if matches!(self.variant, BadgeVariant::Glass) {
            s = s.blur(GLASS_BLUR_RADIUS, GLASS_SATURATION);
        }
        s
    }

    /// Build the pill node (centers the content).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
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
                padding: EdgeInsets::symmetric(FxPx::new(2), FxPx::new(8)),
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
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
    fn variant_colors_from_tokens() {
        let t = BaseTokens;
        assert_eq!(
            Badge::new().variant(BadgeVariant::Destructive).background(&t),
            Some(t.color(ColorToken::Danger))
        );
        assert_eq!(
            Badge::new().variant(BadgeVariant::Success).foreground(&t),
            t.color(ColorToken::OnSuccess)
        );
        // Outline is transparent with a border.
        let outline = Badge::new().variant(BadgeVariant::Outline);
        assert_eq!(outline.background(&t), None);
        assert!(outline.style(&t).visual().border.top.is_some());
    }

    #[test]
    fn glass_is_translucent_and_blurred() {
        let t = BaseTokens;
        let b = Badge::new().variant(BadgeVariant::Glass);
        assert_eq!(b.background(&t).map(|c| c.a), Some(GLASS_FILL_ALPHA));
        assert!(b.style(&t).backdrop_blur().is_some());
    }

    #[test]
    fn builds_a_pill_stack_with_id() {
        let t = BaseTokens;
        match Badge::new().id("count").variant(BadgeVariant::Active).build(&t) {
            LayoutNode::Stack(stack, visual, _) => {
                assert_eq!(stack.id, Some("count"));
                assert_eq!(stack.align, Align::Center);
                assert!(visual.background.is_some());
            }
            _ => panic!("Badge must build a Stack"),
        }
    }
}
