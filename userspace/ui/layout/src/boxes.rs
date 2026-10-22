// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//!
//! CONTEXT: Layout OUTPUT types — the boxes the engine produces and the
//!          scroll-damage arithmetic over them. Split out of `engine.rs`
//!          (RFC-0057 / TASK-0306) so the engine file holds algorithms and
//!          this one holds the data contract the renderer and hit-tester read.
//! OWNERS: @ui
//! STATUS: Done
//! API_STABILITY: Unstable
//! TEST_COVERAGE: engine_tests, nexus-dsl-runtime::interact::hit_slop_tests
//! ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md

use alloc::vec::Vec;
use nexus_layout_types::{FxPx, Overflow, Rect, VisualStyle};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutBox {
    pub node_id: usize,
    pub id: Option<&'static str>,
    pub rect: Rect,
    pub z_index: i16,
    pub visual: VisualStyle,
    /// Optional scissor rect: the intersection of the clipping ancestors' content boxes. Inside
    /// a scroll container's content (RFC-0109) only the clips INSIDE that content count — the
    /// container's viewport clips the content in the compositor, where the content scrolls, so a
    /// box far below the viewport is not clipped away at layout. The renderer clips paint output
    /// to this rect (in layout coordinates) — the box's own paint and its hit area included.
    pub clip_rect: Option<Rect>,
    /// The clip this box imposes on its CONTENT: for `Overflow::Hidden` / `Scroll`, its padding
    /// box within `clip_rect`; `None` for a box that clips nothing. A scroll container's
    /// viewport. Its own background, border and padding are clipped by `clip_rect` alone.
    pub content_clip: Option<Rect>,
    /// The scroll container whose content this box is (its `node_id`; RFC-0109): the box is
    /// drawn into that container's band, at identity, and the compositor moves it by the
    /// container's offset. `None`: the box is the page's base layer. Only a top-level scroll
    /// container is a root — one nested in another's content clips like `Overflow::Hidden`.
    pub scroll_root: Option<usize>,
    /// Scroll offset (dx, dy) for overflow containers. Children are shifted by this
    /// amount relative to the container origin. Non-scrollable boxes have (0, 0).
    pub scroll_offset: (FxPx, FxPx),
    /// The container's overflow mode. Used by renderer to decide whether to apply
    /// scissor clipping and scrollbar rendering.
    pub overflow: Overflow,
    /// `.hitSlop(n)` — outward growth of the INPUT rect only (see
    /// [`nexus_layout_types::FlexItem::hit_slop`]). `rect` stays the painted
    /// rect, so pixels and layout are untouched; only `hit_scrolled` reads it.
    pub hit_slop: FxPx,
    /// Whether an ANCESTOR of this box carries a glass material. The glass
    /// painter keys on it: a glass ROOT resets its rect to the pure tint
    /// (the compositor supplies the blurred backdrop), while NESTED glass
    /// blends src-over onto its parent's tint — without the distinction a
    /// `subtle` row erased the pane beneath it and every glass stack showed
    /// only the wallpaper. Stamped by the engine (it walks the real tree;
    /// geometry alone cannot recover ancestry).
    pub glass_nested: bool,
    /// The font size the LAYOUT chose for this text box under an ancestor's
    /// `.textFit` — `None` when the node keeps its authored `.textSize`.
    ///
    /// It lives on the box and not on the scene node on purpose. `resize()` is
    /// a pure relayout: it rebuilds these boxes and re-walks the UNCHANGED
    /// scene, so a size recorded at emit time could never follow a window
    /// resize. Writing back into the scene would also mean mutating a tree
    /// that `collect_texts`, `path_to_box_id` and the animation keyer each
    /// walk with their own pre-order counters.
    pub text_px: Option<FxPx>,
}

impl Default for LayoutBox {
    /// An inert box at the origin: no id, no clip, no slop, `Overflow::Visible`.
    /// Exists so test fixtures and future construction sites name only the
    /// fields they actually care about (`..LayoutBox::default()`).
    fn default() -> Self {
        Self {
            node_id: 0,
            id: None,
            rect: Rect::zero(),
            z_index: 0,
            visual: VisualStyle::default(),
            clip_rect: None,
            content_clip: None,
            scroll_offset: (FxPx::ZERO, FxPx::ZERO),
            overflow: Overflow::Visible,
            hit_slop: FxPx::ZERO,
            glass_nested: false,
            scroll_root: None,
            text_px: None,
        }
    }
}

/// What the placement of a subtree inherits from its ancestors (stamped on every box it places):
/// whether a glass lies above it, and the scroll container whose content it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlaceCtx {
    pub(crate) in_glass: bool,
    pub(crate) scroll_root: Option<usize>,
}

impl PlaceCtx {
    /// The context of a container's descendants: nested under a glass when the container is one
    /// (the container itself is nested only under an ancestor's).
    pub(crate) fn under(self, material: nexus_layout_types::SurfaceMaterial) -> Self {
        let glass = matches!(material, nexus_layout_types::SurfaceMaterial::Glass(_));
        Self { in_glass: self.in_glass || glass, ..self }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutResult {
    pub boxes: Vec<LayoutBox>,
    pub content_height: FxPx,
}
