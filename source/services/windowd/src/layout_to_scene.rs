// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: **the bridge** (RFC-0067 P4.0) — a laid-out widget tree
//! (`LayoutResult`: a rect + `VisualStyle` per box) → retained scene-graph
//! nodes. This is what makes "a UI is a widget" real: a widget or the DSL
//! produces a `LayoutNode`, `nexus-layout` resolves it to `LayoutBox`es, and
//! this inserts the matching `SceneNode`s under a parent. The `scene_graph`
//! (the render SSOT) then emits the nexus-gfx CommandBuffer. Nothing here draws
//! — it only maps layout → primitives, so windows/panels/apps all render
//! through ONE path instead of hand-rolled `ShellWindow` compositing.
//!
//! Boundary: this belongs to the compositor's *scene assembly* (allowed in
//! windowd); the CHROME it renders is authored in `ui/widgets/window`, not here.
//! See docs/dev/ui/patterns/windowing/windows-as-widgets.md.
//!
//! OWNERS: @ui
//! STATUS: Experimental (RFC-0067 P4.0 — first structural cut; text runs +
//! full parent hierarchy are follow-ups)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests below (pure `LayoutResult` → node list)

// The bridge is the P4.0 foundation; its consumer (the app-client window as a
// `window::Window` LayoutNode) lands in P4.1. Allow until then.
#![allow(dead_code)]

use crate::scene_graph::{RenderPrimitive, SceneGraph, SceneNode, SceneNodeId};
use alloc::vec::Vec;
use nexus_layout::LayoutResult;
use nexus_layout_types::{BoxShadow, GlassLevel, Rect, SurfaceMaterial, VisualStyle};

/// Backdrop blur radius + saturation for each design-system glass level. The
/// compositor supplies the blur; the widget only tags the material.
fn glass_params(level: GlassLevel) -> (u32, u32) {
    match level {
        GlassLevel::Panel => (40, 140),
        GlassLevel::Card => (20, 140),
        GlassLevel::Subtle => (12, 120),
        GlassLevel::Window => (30, 130),
    }
}

/// Uniform corner radius in px (top-left corner is representative; per-corner
/// radii are a later refinement).
fn radius_px(visual: &VisualStyle) -> u32 {
    visual.corner_radius.top_left.0.max(0) as u32
}

fn shadow_of(visual: &VisualStyle) -> Option<BoxShadow> {
    visual.shadow
}

/// Insert one node under `parent`, returning its id.
fn insert(
    graph: &mut SceneGraph,
    parent: SceneNodeId,
    x: i32,
    y: i32,
    primitive: Option<RenderPrimitive>,
    clip: Option<Rect>,
) -> SceneNodeId {
    let id = graph.next_id();
    let mut node = SceneNode::new(id);
    node.parent = Some(parent);
    node.x = x;
    node.y = y;
    node.clip = clip;
    node.primitive = primitive;
    graph.insert(node);
    id
}

/// Bridge a `LayoutResult` into `graph` under `parent`, back-to-front (box list
/// order = z-order). Returns the inserted node ids (parallel to `layout.boxes`).
///
/// Mapping (first cut):
/// - `material: glass(level)` → a `Group` (carrying any shadow) with a
///   `BackdropFilter` child + a `Rect` fill child → the compositor's frosted
///   layer, the same recipe hand-built panels use.
/// - opaque box with a `background` → a filled `Rect`.
/// - a box with neither is a structural container (positions children only).
///
/// Text runs are NOT emitted yet (`RenderPrimitive::Text` is `&'static`; dynamic
/// DSL strings need the interning pool — a follow-up).
pub(crate) fn insert_layout(
    graph: &mut SceneGraph,
    parent: SceneNodeId,
    layout: &LayoutResult,
) -> Vec<SceneNodeId> {
    let mut ids = Vec::with_capacity(layout.boxes.len());
    for b in &layout.boxes {
        let x = b.rect.x.0;
        let y = b.rect.y.0;
        let w = b.rect.width.0.max(0) as u32;
        let h = b.rect.height.0.max(0) as u32;
        let radius = radius_px(&b.visual);

        let id = match b.visual.material {
            SurfaceMaterial::Glass(level) => {
                // Group node carries the shadow + positions the glass; the blur
                // and fill are children so the compositor layers them.
                let group = insert(
                    graph,
                    parent,
                    x,
                    y,
                    Some(RenderPrimitive::Group { shadow: shadow_of(&b.visual) }),
                    b.clip_rect,
                );
                let (blur, sat) = glass_params(level);
                let _ = insert(
                    graph,
                    group,
                    0,
                    0,
                    Some(RenderPrimitive::BackdropFilter {
                        blur_radius: blur,
                        saturation_percent: sat,
                    }),
                    None,
                );
                if let Some(bg) = b.visual.background {
                    let _ = insert(
                        graph,
                        group,
                        0,
                        0,
                        Some(RenderPrimitive::Rect { width: w, height: h, radius, color: bg }),
                        None,
                    );
                }
                group
            }
            SurfaceMaterial::Opaque => {
                let prim = b.visual.background.map(|bg| RenderPrimitive::Rect {
                    width: w,
                    height: h,
                    radius,
                    color: bg,
                });
                insert(graph, parent, x, y, prim, b.clip_rect)
            }
        };
        ids.push(id);
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout::LayoutBox;
    use nexus_layout_types::{CornerRadius, FxPx, Overflow, Rgba8, VisualStyle};

    fn boxed(rect: Rect, visual: VisualStyle) -> LayoutBox {
        LayoutBox {
            node_id: 0,
            id: None,
            rect,
            z_index: 0,
            visual,
            clip_rect: None,
            scroll_offset: (FxPx::ZERO, FxPx::ZERO),
            overflow: Overflow::Visible,
        }
    }

    #[test]
    fn opaque_bg_box_becomes_one_rect() {
        let mut visual = VisualStyle::default();
        visual.background = Some(Rgba8::new(10, 20, 30, 255));
        visual.corner_radius = CornerRadius::uniform(FxPx::new(8));
        let layout = LayoutResult {
            boxes: alloc::vec![boxed(
                Rect::new(FxPx::new(4), FxPx::new(6), FxPx::new(100), FxPx::new(40)),
                visual
            )],
            content_height: FxPx::new(40),
        };
        let mut graph = SceneGraph::new();
        let root = graph.next_id();
        graph.insert(SceneNode::new(root));
        let ids = insert_layout(&mut graph, root, &layout);
        assert_eq!(ids.len(), 1);
        let node = graph.find(ids[0]).expect("node");
        assert_eq!((node.x, node.y), (4, 6));
        assert!(matches!(
            node.primitive,
            Some(RenderPrimitive::Rect { width: 100, height: 40, radius: 8, .. })
        ));
    }

    #[test]
    fn glass_box_becomes_group_with_backdrop_and_fill() {
        let mut visual = VisualStyle::default();
        visual.material = SurfaceMaterial::Glass(GlassLevel::Panel);
        visual.background = Some(Rgba8::new(255, 255, 255, 40));
        let layout = LayoutResult {
            boxes: alloc::vec![boxed(
                Rect::new(FxPx::ZERO, FxPx::ZERO, FxPx::new(200), FxPx::new(48)),
                visual
            )],
            content_height: FxPx::new(48),
        };
        let mut graph = SceneGraph::new();
        let root = graph.next_id();
        graph.insert(SceneNode::new(root));
        let before = graph.node_count();
        let ids = insert_layout(&mut graph, root, &layout);
        // group + backdrop + fill = 3 new nodes.
        assert_eq!(graph.node_count() - before, 3);
        assert!(matches!(
            graph.find(ids[0]).unwrap().primitive,
            Some(RenderPrimitive::Group { .. })
        ));
        // The group has a BackdropFilter child (Panel = blur 40).
        let has_backdrop = graph.children(ids[0]).any(|c| {
            matches!(
                graph.find(c).unwrap().primitive,
                Some(RenderPrimitive::BackdropFilter { blur_radius: 40, .. })
            )
        });
        assert!(has_backdrop, "glass group must carry a backdrop-blur child");
    }
}
