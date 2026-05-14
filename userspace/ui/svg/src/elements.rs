// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::math::F32Math;
use alloc::string::String as AString;
use alloc::vec::Vec as AVec;
use hashbrown::HashMap;

/// Parsed SVG document.
#[derive(Debug, Clone)]
pub struct SvgDocument {
    pub width: f32,
    pub height: f32,
    pub elements: AVec<SvgElement>,
    pub defs: HashMap<AString, SvgElement>,
}

/// An SVG element in the parsed tree.
#[derive(Debug, Clone)]
pub enum SvgElement {
    Group {
        children: AVec<SvgElement>,
        transform: Option<Transform>,
        opacity: f32,
    },
    Path {
        data: PathData,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_width: f32,
        transform: Option<Transform>,
        opacity: f32,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rx: f32,
        ry: f32,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_width: f32,
        transform: Option<Transform>,
        opacity: f32,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_width: f32,
        transform: Option<Transform>,
        opacity: f32,
    },
    Ellipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_width: f32,
        transform: Option<Transform>,
        opacity: f32,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        stroke: Option<Paint>,
        stroke_width: f32,
        transform: Option<Transform>,
        opacity: f32,
    },
    Polygon {
        points: AVec<(f32, f32)>,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_width: f32,
        transform: Option<Transform>,
        opacity: f32,
    },
    LinearGradient {
        id: AString,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        stops: AVec<GradientStop>,
    },
}

/// A gradient color stop.
#[derive(Debug, Clone)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}

/// Paint for filling or stroking.
#[derive(Debug, Clone)]
pub enum Paint {
    Color(Color),
    /// Reference to a gradient by ID (internal only).
    GradientRef(AString),
    None,
}

/// An RGBA color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Color = Color { r: 255, g: 255, b: 255, a: 255 };
}

/// Parsed SVG path data.
#[derive(Debug, Clone)]
pub struct PathData {
    pub commands: AVec<PathCommand>,
    pub fill_rule: FillRule,
}

/// A single path command.
#[derive(Debug, Clone)]
pub enum PathCommand {
    MoveTo { x: f32, y: f32 },
    MoveToRel { dx: f32, dy: f32 },
    LineTo { x: f32, y: f32 },
    LineToRel { dx: f32, dy: f32 },
    HorizontalTo { x: f32 },
    HorizontalToRel { dx: f32 },
    VerticalTo { y: f32 },
    VerticalToRel { dy: f32 },
    CubicTo { x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32 },
    CubicToRel { dx1: f32, dy1: f32, dx2: f32, dy2: f32, dx: f32, dy: f32 },
    SmoothCubicTo { x2: f32, y2: f32, x: f32, y: f32 },
    SmoothCubicToRel { dx2: f32, dy2: f32, dx: f32, dy: f32 },
    QuadraticTo { x1: f32, y1: f32, x: f32, y: f32 },
    QuadraticToRel { dx1: f32, dy1: f32, dx: f32, dy: f32 },
    SmoothQuadraticTo { x: f32, y: f32 },
    SmoothQuadraticToRel { dx: f32, dy: f32 },
    ArcTo { rx: f32, ry: f32, xrot: f32, large: bool, sweep: bool, x: f32, y: f32 },
    ArcToRel { rx: f32, ry: f32, xrot: f32, large: bool, sweep: bool, dx: f32, dy: f32 },
    ClosePath,
}

/// Fill rule for path rendering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// 2D affine transform.
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Transform {
    pub const IDENTITY: Transform = Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn translate(tx: f32, ty: f32) -> Self {
        Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: tx, f: ty }
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Transform { a: sx, b: 0.0, c: 0.0, d: sy, e: 0.0, f: 0.0 }
    }

    pub fn rotate(angle_deg: f32) -> Self {
        let rad = angle_deg.nexus_to_radians();
        let (s, c) = rad.nexus_sin_cos();
        Transform { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    pub fn compose(&self, other: &Transform) -> Transform {
        Transform {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform::IDENTITY
    }
}
