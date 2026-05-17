// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::color::Rgba8;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeKind {
    Rect,
    Circle,
    TriangleUp,
    TriangleDown,
    Path(PathShape),
}

impl Default for ShapeKind {
    fn default() -> Self {
        Self::Rect
    }
}

/// Visual style attached to container and text nodes.
/// Separated from layout properties — paint-only changes don't invalidate measurement.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisualStyle {
    pub background: Option<Rgba8>,
    pub border: EdgeBorder,
    pub corner_radius: CornerRadius,
    pub opacity: Option<FxPx>,
    pub shape: ShapeKind,
}
