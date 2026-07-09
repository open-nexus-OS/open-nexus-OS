// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: **the app-client window AS A WIDGET** (RFC-0067 P4.1). The window
//! manager (windowd) composes a client window's chrome by instantiating the
//! `nexus-widget-window` `Window` (title bar + body) — it does NOT hand-draw a
//! frame. The result is a `LayoutNode`; `nexus-layout` resolves it and
//! `layout_to_scene` turns it into scene nodes the compositor renders. This is
//! the path that RETIRES the legacy `compositor/shell_window.rs` for app
//! windows: chrome, sizing, materials, and (next) resize all live in the widget,
//! not in windowd.
//!
//! Boundary: building the FRAME here is the WM's job (compose chrome per
//! `intent ⟂ policy`); the frame's STRUCTURE is authored in the widget crate.
//! windowd still owns only surfaces/damage/present. See
//! docs/dev/ui/patterns/windowing/windows-as-widgets.md.
//!
//! OWNERS: @ui
//! STATUS: Experimental (RFC-0067 P4.1 — chain host-tested; the live scene-graph
//! render for this window is the boot-gated follow-up)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host test below (build → layout → bridge → scene).

#![allow(dead_code)]

use nexus_layout_types::{
    FlexItem, FxPx, LayoutNode, Spacer, TextContent, TextNode, TextStyle, VisualStyle,
};
use nexus_widget_window::Window;

/// A title-bar text node (the WM labels the window; the app never draws chrome).
fn title_text(title: &str) -> LayoutNode {
    LayoutNode::Text(
        TextNode {
            id: None,
            content: TextContent::new(title),
            style: TextStyle::default(),
            item: FlexItem::default(),
            max_lines: Some(1),
            min_width: None,
            max_width: None,
        },
        VisualStyle::default(),
    )
}

/// The body slot standing in for the app's client surface. windowd fills this
/// region from the app's VMO (`Surface` primitive) when the scene renders; here
/// it only reserves the space in the layout (a grow-spacer).
fn body_slot() -> LayoutNode {
    LayoutNode::Spacer(Spacer {
        id: Some("app-window-body"),
        flex_grow: 1,
        min_size: Some(FxPx::new(1)),
        item: FlexItem::default(),
    })
}

/// Builds the app-client window's chrome as a `Window` widget `LayoutNode`
/// (title bar + body). The window MANAGER composes this; the client only
/// provides content (which lands in the body slot). No `ShellWindow`.
pub(crate) fn app_window_node(title: &str) -> LayoutNode {
    Window::new()
        .id("app-window")
        .titlebar_id("app-window-titlebar")
        .title(title_text(title))
        .body(alloc::vec![body_slot()])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_graph::{SceneGraph, SceneNode};
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::{
        FxPx, LineLayout, LineMetrics, MeasureText, PreparedTextHandle, TextContent, TextStyle,
    };

    /// Trivial fixed-advance measurer (the OS uses the baked-font harness; the
    /// scene structure is font-independent, so a mock keeps the test pure).
    struct MockMeasure;
    impl MeasureText for MockMeasure {
        fn prepare(&self, _content: &TextContent, _style: &TextStyle) -> PreparedTextHandle {
            PreparedTextHandle(0)
        }
        fn measure_width(&self, _handle: &PreparedTextHandle) -> FxPx {
            FxPx::new(40)
        }
        fn layout_lines(
            &self,
            _handle: &PreparedTextHandle,
            _width: FxPx,
            _max_lines: Option<u32>,
        ) -> LineLayout {
            LineLayout {
                lines: alloc::vec![LineMetrics {
                    text_range: 0..1,
                    width: FxPx::new(40),
                    baseline: FxPx::new(12),
                    height: FxPx::new(16),
                }],
                natural_width: FxPx::new(40),
            }
        }
    }

    #[test]
    fn app_window_builds_layouts_and_bridges_to_scene() {
        // widget → LayoutNode
        let node = app_window_node("App");
        // LayoutNode → LayoutResult (content-sized layout, no fixed max)
        let layout = LayoutEngine::new()
            .layout(&node, FxPx::new(320), &MockMeasure)
            .expect("app window lays out");
        assert!(!layout.boxes.is_empty(), "window produced boxes");
        // LayoutResult → scene nodes (the P4.0 bridge)
        let mut graph = SceneGraph::new();
        let root = graph.next_id();
        graph.insert(SceneNode::new(root));
        let before = graph.node_count();
        let ids = crate::layout_to_scene::insert_layout(&mut graph, root, &layout);
        assert_eq!(ids.len(), layout.boxes.len());
        assert!(graph.node_count() > before, "window inserted scene nodes");
    }
}
