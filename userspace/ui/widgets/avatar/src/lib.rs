// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Avatar` — the design-system avatar (handoff `Avatar`): a circular (or
//! rounded-square) image with an initials fallback on a glass backing, plus an
//! optional presence status. A pure builder producing a `LayoutNode::Stack`
//! tile; the presence dot is exposed as data (a corner overlay the compositor
//! places, never clipped by the tile). DSL-emittable.

extern crate alloc;

use alloc::string::String;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack,
};
use nexus_style::Style;
use nexus_layout_types::FontWeight;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};
use nexus_widget_text::Text;

/// Presence status (maps to a dot color).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarStatus {
    Online,
    Busy,
    Away,
    Offline,
}

impl AvatarStatus {
    /// The semantic color role for the presence dot.
    pub fn color(self) -> ColorToken {
        match self {
            AvatarStatus::Online => ColorToken::Success,
            AvatarStatus::Busy => ColorToken::Danger,
            AvatarStatus::Away => ColorToken::Warning,
            AvatarStatus::Offline => ColorToken::OnSurfaceVariant,
        }
    }
}

/// An avatar tile.
#[derive(Debug, Clone)]
pub struct Avatar {
    initials: Option<String>,
    image: Option<LayoutNode>,
    size: i32,
    square: bool,
    status: Option<AvatarStatus>,
    id: Option<&'static str>,
}

impl Default for Avatar {
    fn default() -> Self {
        Self { initials: None, image: None, size: 40, square: false, status: None, id: None }
    }
}

impl Avatar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Initials shown when there is no image.
    pub fn initials(mut self, initials: impl Into<String>) -> Self {
        self.initials = Some(initials.into());
        self
    }
    /// Image node (caller-decoded).
    pub fn image(mut self, image: LayoutNode) -> Self {
        self.image = Some(image);
        self
    }
    pub fn size(mut self, size: i32) -> Self {
        self.size = size.max(16);
        self
    }
    /// Rounded-square instead of circle.
    pub fn square(mut self, square: bool) -> Self {
        self.square = square;
        self
    }
    pub fn status(mut self, status: AvatarStatus) -> Self {
        self.status = Some(status);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The presence status to render as a corner overlay (`None` = hidden).
    pub fn presence(&self) -> Option<AvatarStatus> {
        self.status
    }

    /// Build the avatar tile (image or initials on a glass backing).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let radius = if self.square {
            tokens.length(LengthToken::RadiusMedium)
        } else {
            FxPx::new(self.size / 2)
        };
        let style = Style::new().background(tokens.color(ColorToken::SurfaceVariant)).rounded(radius);

        let content = match (self.image, &self.initials) {
            (Some(image), _) => image,
            (None, Some(initials)) => {
                Text::new(initials.clone()).weight(FontWeight::Medium).build(tokens)
            }
            (None, None) => LayoutNode::Spacer(nexus_layout_types::Spacer {
                id: None,
                flex_grow: 0,
                min_size: None,
                item: FlexItem::default(),
            }),
        };

        let d = Some(FxPx::new(self.size));
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem::default(),
            },
            style.visual(),
            alloc::vec![content],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn circle_size_and_glass_backing() {
        let t = BaseTokens;
        match Avatar::new().initials("LK").size(48).build(&t) {
            LayoutNode::Stack(stack, v, children) => {
                assert_eq!(stack.min_width, Some(FxPx::new(48)));
                assert_eq!(v.corner_radius.top_left, FxPx::new(24), "circle = size/2");
                assert_eq!(v.background, Some(t.color(ColorToken::SurfaceVariant)));
                assert_eq!(children.len(), 1, "the initials");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn status_maps_to_a_color_and_is_exposed() {
        assert_eq!(AvatarStatus::Online.color(), ColorToken::Success);
        assert_eq!(Avatar::new().status(AvatarStatus::Busy).presence(), Some(AvatarStatus::Busy));
        assert_eq!(Avatar::new().presence(), None);
    }
}
