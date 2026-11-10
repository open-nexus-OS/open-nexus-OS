// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Visual style types (borders, shadows, shapes) for TASK-0058/TASK-0059 / RFC-0057/RFC-0058.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 5 tests (tests/ui_v4_host/src/lib.rs — VisualStyle extensions)
//! ADR: docs/adr/0030-layout-engine-deterministic-pretext.md
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

/// Text shadow descriptor. Simpler than box shadow — no spread, just offset + blur + color. A
/// text shadow reaches a few pixels past its glyphs, so its geometry is whole pixels in a byte
/// each: every node of every frame carries two of these ([`VisualStyle::text_shadows`]), and the
/// pair costs less than one wide descriptor did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextShadow {
    /// Horizontal offset in layout pixels.
    pub offset_x: i8,
    /// Vertical offset in layout pixels.
    pub offset_y: i8,
    /// Blur radius in layout pixels, as CSS reads it (a Gaussian of σ = radius / 2): 0 = sharp.
    pub blur_radius: u8,
    /// Shadow color.
    pub color: Rgba8,
}

impl TextShadow {
    /// The label halo in `color` (RFC-0116 `labelShadow`), as the design artifacts draw it behind
    /// a label on the wallpaper: a 3 px glow around the glyphs, and under it the same glow one
    /// pixel lower at four fifths of the alpha (`0 0 3px c, 0 1px 3px c·0.8`) — front first, the
    /// order of [`VisualStyle::text_shadows`].
    #[must_use]
    pub const fn halo(color: Rgba8) -> [Self; 2] {
        let glow = Self { offset_x: 0, offset_y: 0, blur_radius: 3, color };
        let lower = Rgba8 { a: (color.a as u16 * 4 / 5) as u8, ..color };
        [glow, Self { offset_y: 1, color: lower, ..glow }]
    }
}

impl Default for TextShadow {
    fn default() -> Self {
        Self { offset_x: 0, offset_y: 2, blur_radius: 4, color: Rgba8 { r: 0, g: 0, b: 0, a: 80 } }
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
    /// A single filled contour (normalized `0..1000` viewbox).
    Path(PathShape),
    /// A multi-contour vector symbol (each contour a filled sub-path).
    Vector(alloc::vec::Vec<PathShape>),
    /// A line symbol: polylines (normalized `0..1000` viewbox; `closed` adds the segment from the
    /// last point back to the first) stroked `width_milli` wide (same viewbox units) with round
    /// caps and round joins — the imported line-icon set's own stroke model. Painted as the
    /// anti-aliased union of its segments' capsules: joins and overlaps are painted once.
    Stroke {
        paths: alloc::vec::Vec<PathShape>,
        width_milli: u16,
    },
    /// A pre-rasterized straight-alpha RGBA sprite (build-time baked app-icon
    /// artwork). `w × h` pixels, `[r,g,b,a]` per pixel, row-major; the
    /// painter samples nearest, so any box size renders.
    Raster {
        w: u16,
        h: u16,
        rgba: &'static [u8],
    },
}

/// The material of a surface (RFC-0116): which colour role it is filled with and whether it
/// blurs what lies behind it. Only the root surface of a stack blurs (`Panel`, `Window`, `Dock`,
/// `Thin`, `UltraThin` on the wallpaper; `Overlay`, everything that floats, over what it floats
/// on); `Card` is a translucent layer on such a surface and never blurs; `Pane` (inside a
/// window) is opaque, and so is an `Overlay` nested in another surface (a floating action bar
/// on a pane), which cannot blur there. The compositor turns a blurring root into a
/// backdrop-blurred layer, an opaque surface into an occluder; the app never blurs anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Material {
    /// A panel on the wallpaper (Control Center, notifications, launcher): glassPanel, regular blur.
    Panel,
    /// An app window: glassWindow, regular blur.
    Window,
    /// The dock, the taskbar, the launcher backdrop: glassPanel, thick blur.
    Dock,
    /// A light standalone piece on the wallpaper: glassPanel, thin blur.
    Thin,
    /// A small standalone piece on the wallpaper: glassPanel, ultra-thin blur.
    UltraThin,
    /// A card, a grouped row or a tile on a blur surface: glassCard, no blur.
    Card,
    /// The opaque pane inside a window: surfacePane.
    Pane,
    /// A floating surface (modal, alert, menu, popover, toast): glassOverlay, regular blur — a
    /// dense frosted reading surface as a root; nested in another surface, opaque.
    Overlay,
}

impl Material {
    /// Whether the compositor blurs the backdrop under this surface.
    #[must_use]
    pub const fn blurs(self) -> bool {
        matches!(
            self,
            Self::Panel | Self::Window | Self::Dock | Self::Thin | Self::UltraThin | Self::Overlay
        )
    }

    /// Whether the surface reads as opaque: a pane, and an overlay — frosted as a root, an
    /// occluder where it is nested (it cannot blur there). A card on it takes the pane's fill.
    #[must_use]
    pub const fn opaque(self) -> bool {
        matches!(self, Self::Pane | Self::Overlay)
    }
}

/// Visual style attached to container and text nodes.
/// Separated from layout properties — paint-only changes don't invalidate measurement.
///
/// Phase 6a: added `shadow`, `text_shadows`. `opacity` is a 0-255 fraction
/// (0 = fully transparent, 255 = fully opaque).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisualStyle {
    /// Background fill color (None = transparent).
    pub background: Option<Rgba8>,
    /// Vertical linear background gradient `(top, bottom)` — the design
    /// system's `linear-gradient(to bottom, …)`. Wins over `background`
    /// when set; the row-based painter lerps one flat color per row.
    pub background_gradient: Option<(Rgba8, Rgba8)>,
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
    /// Up to two text shadows cast by the glyphs of this node, front first (CSS order): the
    /// painter draws the second, then the first, then the text.
    pub text_shadows: [Option<TextShadow>; 2],
    /// The design system's `inset 0 1px 0 <color>` top-shine: a one-pixel
    /// highlight drawn just INSIDE the top edge (RFC-0082). This is what makes
    /// a glass surface read as a lit pane rather than a flat tint, and it is
    /// deliberately separate from `background_gradient` — the same authored
    /// `edgeHighlight` alpha is a crisp line here and, at a fraction of its
    /// strength, a soft wash there.
    pub inset_highlight: Option<Rgba8>,
    /// The surface's material (RFC-0116); `None` = a plain painted box.
    pub material: Option<Material>,
}
