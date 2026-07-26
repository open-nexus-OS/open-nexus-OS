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
use nexus_layout_types::FontWeight;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, MaterialToken, Tokens, TypographyToken};
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

/// Initials for a display name: the first letter of up to two whitespace-
/// separated words, uppercased. Empty input yields an empty string — the
/// avatar then renders as a bare backing, which is honest for "no user yet".
#[must_use]
pub fn initials_of(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
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
    material: Option<MaterialToken>,
    fg: Option<ColorToken>,
    type_size: Option<TypographyToken>,
}

impl Default for Avatar {
    fn default() -> Self {
        Self {
            initials: None,
            image: None,
            size: 40,
            square: false,
            status: None,
            id: None,
            material: None,
            fg: None,
            type_size: None,
        }
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

    /// Initials DERIVED from a display name — the first letter of up to two
    /// words, uppercased ("Jenning Schäfer" → "JS", "Jenning" → "J").
    ///
    /// Deriving here rather than at the call site is what keeps every avatar
    /// in the OS consistent: the DSL has no string operations, so the
    /// alternative is each app inventing its own rule or the wire carrying a
    /// second field.
    pub fn name(mut self, name: &str) -> Self {
        self.initials = Some(initials_of(name));
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

    /// A glass backing at `level` instead of the flat `SurfaceVariant` fill —
    /// what an avatar needs when it sits on a wallpaper rather than in a list.
    pub fn material(mut self, level: MaterialToken) -> Self {
        self.material = Some(level);
        self
    }

    /// Ink color for the initials (defaults to `OnSurface`).
    pub fn fg(mut self, color: ColorToken) -> Self {
        self.fg = Some(color);
        self
    }

    /// Font size of the initials (defaults to the widget's own scaling).
    pub fn type_size(mut self, token: TypographyToken) -> Self {
        self.type_size = Some(token);
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
        let style = match self.material {
            Some(level) => Style::new().glass(level, tokens).rounded(radius),
            None => {
                Style::new().background(tokens.color(ColorToken::SurfaceVariant)).rounded(radius)
            }
        };

        let content = match (self.image, &self.initials) {
            (Some(image), _) => image,
            (None, Some(initials)) => {
                let mut text = Text::new(initials.clone()).weight(FontWeight::Semibold);
                if let Some(size) = self.type_size {
                    text = text.size(size);
                }
                if let Some(fg) = self.fg {
                    text = text.color(fg);
                }
                text.build(tokens)
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
    fn initials_come_from_the_display_name() {
        assert_eq!(initials_of("Jenning Schäfer"), "JS");
        assert_eq!(initials_of("Jenning"), "J");
        assert_eq!(initials_of("Ada Byron Lovelace"), "AB", "at most two words");
        assert_eq!(initials_of("  spaced   out  "), "SO");
        assert_eq!(initials_of("ätna öl"), "ÄÖ", "uppercasing is not ASCII-only");
        assert_eq!(initials_of(""), "", "no user yet renders a bare backing");
    }

    #[test]
    fn material_swaps_the_flat_fill_for_glass() {
        let t = BaseTokens;
        // Default: the flat list-row disc.
        match Avatar::new().name("Jenning").build(&t) {
            LayoutNode::Stack(_, v, _) => {
                assert_eq!(v.background, Some(t.color(ColorToken::SurfaceVariant)));
                assert_eq!(v.material, nexus_layout_types::SurfaceMaterial::Opaque);
            }
            _ => panic!(),
        }
        // On a wallpaper: real glass, with the material's tint and hairline.
        match Avatar::new().name("Jenning").material(MaterialToken::Card).build(&t) {
            LayoutNode::Stack(_, v, _) => {
                assert_eq!(
                    v.material,
                    nexus_layout_types::SurfaceMaterial::Glass(
                        nexus_layout_types::GlassLevel::Card
                    )
                );
                assert_eq!(v.background, Some(t.glass(MaterialToken::Card).tint));
                assert!(v.inset_highlight.is_some(), "glass carries the inset top-shine");
                assert!(v.border.top.is_some(), "glass carries the 1px hairline");
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
