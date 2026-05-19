// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::color::Rgba8;
use crate::node::Fraction;
use crate::types::FxPx;
use alloc::vec::Vec;

/// A border edge: width in layout pixels and color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Border {
    pub width: FxPx,
    pub color: Rgba8,
}

/// Per-edge borders. `None` means no border on that edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeBorder {
    pub top: Option<Border>,
    pub right: Option<Border>,
    pub bottom: Option<Border>,
    pub left: Option<Border>,
}

impl EdgeBorder {
    pub const fn all(width: FxPx, color: Rgba8) -> Self {
        let b = Some(Border { width, color });
        EdgeBorder { top: b, right: b, bottom: b, left: b }
    }

    pub const fn bottom(width: FxPx, color: Rgba8) -> Self {
        EdgeBorder { top: None, right: None, bottom: Some(Border { width, color }), left: None }
    }

    pub const fn none() -> Self {
        EdgeBorder { top: None, right: None, bottom: None, left: None }
    }
}

/// Per-corner radii.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CornerRadius {
    pub top_left: FxPx,
    pub top_right: FxPx,
    pub bottom_right: FxPx,
    pub bottom_left: FxPx,
}

impl CornerRadius {
    pub const fn uniform(v: FxPx) -> Self {
        CornerRadius { top_left: v, top_right: v, bottom_right: v, bottom_left: v }
    }

    pub const fn top(v: FxPx) -> Self {
        CornerRadius {
            top_left: v,
            top_right: v,
            bottom_right: FxPx::ZERO,
            bottom_left: FxPx::ZERO,
        }
    }

    pub const fn none() -> Self {
        CornerRadius::uniform(FxPx::ZERO)
    }
}

/// Box shadow descriptor. Defines an outer shadow cast by a rectangular element.
/// The shadow is rendered as an alpha mask offset by (offset_x, offset_y), expanded
/// by `spread`, blurred by `blur_radius`, and tinted with `color`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxShadow {
    /// Horizontal offset in layout pixels (positive = right).
    pub offset_x: FxPx,
    /// Vertical offset in layout pixels (positive = down).
    pub offset_y: FxPx,
    /// Blur radius in layout pixels. 0 = sharp shadow, 1+ = progressive softening.
    pub blur_radius: FxPx,
    /// Spread radius (positive = expand, negative = contract). 0 = same size as element.
    pub spread: FxPx,
    /// Shadow color (RGBA, alpha controls opacity).
    pub color: Rgba8,
}

impl Default for BoxShadow {
    fn default() -> Self {
        Self {
            offset_x: FxPx::ZERO,
            offset_y: FxPx::new(4),
            blur_radius: FxPx::new(8),
            spread: FxPx::ZERO,
            color: Rgba8 { r: 0, g: 0, b: 0, a: 64 },
        }
    }
}

/// Text shadow descriptor. Simpler than box shadow — no spread, just offset + blur + color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextShadow {
    /// Horizontal offset in layout pixels.
    pub offset_x: FxPx,
    /// Vertical offset in layout pixels.
    pub offset_y: FxPx,
    /// Blur radius in layout pixels. 0 = sharp, 1+ = softening.
    pub blur_radius: FxPx,
    /// Shadow color.
    pub color: Rgba8,
}

impl Default for TextShadow {
    fn default() -> Self {
        Self {
            offset_x: FxPx::ZERO,
            offset_y: FxPx::new(2),
            blur_radius: FxPx::new(4),
            color: Rgba8 { r: 0, g: 0, b: 0, a: 80 },
        }
    }
}

/// Pre-defined shadow levels matching common UI densities.
/// Each variant maps to a canonical `BoxShadow` via `to_box_shadow()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowLevel {
    /// sm: subtle elevation (2px blur, 1px offset)
    Sm,
    /// md: moderate elevation (6px blur, 2px offset) — default
    Md,
    /// lg: pronounced elevation (12px blur, 4px offset)
    Lg,
    /// xl: heavy elevation (20px blur, 6px offset)
    Xl,
    /// 2xl: dramatic elevation (32px blur, 10px offset)
    Xxl2,
}

impl ShadowLevel {
    /// Convert a shadow level to its canonical `BoxShadow`.
    pub const fn to_box_shadow(self) -> BoxShadow {
        match self {
            Self::Sm => BoxShadow {
                offset_x: FxPx::ZERO,
                offset_y: FxPx::new(1),
                blur_radius: FxPx::new(2),
                spread: FxPx::ZERO,
                color: Rgba8 { r: 0, g: 0, b: 0, a: 32 },
            },
            Self::Md => BoxShadow {
                offset_x: FxPx::ZERO,
                offset_y: FxPx::new(2),
                blur_radius: FxPx::new(6),
                spread: FxPx::new(-2),
                color: Rgba8 { r: 0, g: 0, b: 0, a: 48 },
            },
            Self::Lg => BoxShadow {
                offset_x: FxPx::ZERO,
                offset_y: FxPx::new(4),
                blur_radius: FxPx::new(12),
                spread: FxPx::new(-3),
                color: Rgba8 { r: 0, g: 0, b: 0, a: 56 },
            },
            Self::Xl => BoxShadow {
                offset_x: FxPx::ZERO,
                offset_y: FxPx::new(6),
                blur_radius: FxPx::new(20),
                spread: FxPx::new(-4),
                color: Rgba8 { r: 0, g: 0, b: 0, a: 64 },
            },
            Self::Xxl2 => BoxShadow {
                offset_x: FxPx::ZERO,
                offset_y: FxPx::new(10),
                blur_radius: FxPx::new(32),
                spread: FxPx::new(-6),
                color: Rgba8 { r: 0, g: 0, b: 0, a: 80 },
            },
        }
    }
}

impl Default for ShadowLevel {
    fn default() -> Self {
        Self::Md
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathPoint {
    pub x_milli: u16,
    pub y_milli: u16,
}

impl PathPoint {
    pub const fn new(x_milli: u16, y_milli: u16) -> Self {
        Self { x_milli, y_milli }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PathShape {
    pub points: Vec<PathPoint>,
    pub closed: bool,
}

impl PathShape {
    pub fn line(points: &[PathPoint]) -> Self {
        Self { points: points.to_vec(), closed: false }
    }

    pub fn polygon(points: &[PathPoint]) -> Self {
        Self { points: points.to_vec(), closed: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ShapeKind {
    #[default]
    Rect,
    Circle,
    TriangleUp,
    TriangleDown,
    Path(PathShape),
}

/// Visual style attached to container and text nodes.
/// Separated from layout properties — paint-only changes don't invalidate measurement.
///
/// Phase 6a: added `shadow`, `text_shadow`. `opacity` is a 0-255 fraction
/// (0 = fully transparent, 255 = fully opaque).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisualStyle {
    /// Background fill color (None = transparent).
    pub background: Option<Rgba8>,
    /// Per-edge borders.
    pub border: EdgeBorder,
    /// Per-corner radii for rounded rectangles.
    pub corner_radius: CornerRadius,
    /// Opacity: 0 = fully transparent, 255 = fully opaque. None = opaque (255).
    pub opacity: Option<Fraction>,
    /// Shape type (Rect, Circle, TriangleUp, TriangleDown, Path).
    pub shape: ShapeKind,
    /// Box shadow (outer shadow cast by the element bounds).
    pub shadow: Option<BoxShadow>,
    /// Text shadow (shadow cast by text glyphs on this node).
    pub text_shadow: Option<TextShadow>,
}