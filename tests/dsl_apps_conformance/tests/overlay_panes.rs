// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The window kit's overlay panes (a narrow window's sidebar sliding out) against the REAL
//! compiled Settings. A bare `.overlay()` is a layer the host must know: app-host paints a scroll
//! layer that lies under one in the base, under it — while the runtime registered only modal and
//! transient layers, the content's scroll layer was composed OVER the sidebar pane on the board
//! (its text printed over the pane, board cycle 2026-11-09). And the pane slides out over the
//! content undimmed: no scrim behind it (the operator's call).

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_layout_types::{LayoutNode, Position};
use nexus_theme_tokens::BaseTokens;

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(&mut self, _: &str, _: &str, _: &[Value]) -> Result<Value, u32> {
        Err(1)
    }
}

#[test]
fn a_sidebar_overlay_is_a_bare_layer_over_the_content_and_dims_nothing() {
    let nxir = common::compile("settings");
    let symbols = common::program_symbols(&nxir);
    let device = FixtureEnv::tablet("portrait"); // the regular tier: the sidebar is an overlay
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
    let closed = view.overlays().plain().len();

    common::dispatch(&mut view, &device, &mut NoHost, &symbols, "WindowEvent", "WinLeft", vec![]);
    let boxes = common::layout_boxes(&view);
    let plain = view.overlays().plain();
    assert_eq!(plain.len(), closed + 1, "the sidebar overlay is registered as a bare layer");
    let layer = plain.last().expect("the overlay");
    let rect = boxes.iter().find(|b| b.node_id == layer.box_id).expect("its box resolves").rect;
    assert_eq!(view.overlays().modal_depth(), 0, "a bare layer confines nothing");

    // It lies over the content's scroll viewport — what app-host's cover check needs.
    let content = boxes
        .iter()
        .filter(|b| matches!(b.overflow, nexus_layout_types::Overflow::Scroll(_)))
        .filter(|b| b.node_id < layer.box_id)
        .find(|b| b.rect.x.0 < rect.x.0 + rect.width.0 && rect.x.0 < b.rect.x.0 + b.rect.width.0)
        .expect("a scroll viewport under the overlay");
    assert!(content.rect.y.0 < rect.y.0 + rect.height.0, "the overlay overlaps the content");

    // Undimmed: the full-bleed layer carries no fill of its own.
    let layer_box = boxes.iter().find(|b| b.node_id == layer.box_id).expect("layer box");
    assert_eq!(layer_box.visual.background, None, "no scrim behind the pane");
}

/// The pre-order span `[first, end)` of node `target`'s subtree — the layout's numbering (root 1,
/// a stack's in-flow children before its absolute ones), as app-host's `subtree_span` counts.
fn subtree_span(root: &LayoutNode, target: usize) -> Option<(usize, usize)> {
    fn walk(node: &LayoutNode, index: &mut usize, target: usize) -> Option<(usize, usize)> {
        *index += 1;
        let here = *index;
        let found = match node {
            LayoutNode::Stack(_, _, children) => {
                let absolute = |c: &&LayoutNode| matches!(c.item().position, Position::Absolute);
                let order = children
                    .iter()
                    .filter(|c| !absolute(c))
                    .chain(children.iter().filter(absolute));
                order.into_iter().find_map(|c| walk(c, index, target))
            }
            LayoutNode::Grid(_, _, children) => {
                children.iter().find_map(|c| walk(c, index, target))
            }
            _ => None,
        };
        found.or_else(|| (here == target).then_some((here, *index + 1)))
    }
    walk(root, &mut 0, target)
}

/// Whether some bare or kinded overlay PAINTS over `viewport` (a fill, a material or a border of
/// its subtree outside any scroll band) — app-host's cover rule.
fn painted_over(
    view: &View<'_>,
    boxes: &[nexus_layout::LayoutBox],
    viewport: (i32, i32, i32, i32),
) -> bool {
    let overlays = view.overlays();
    overlays.entries().iter().chain(overlays.plain()).any(|e| {
        let Some((first, end)) = subtree_span(view.scene(), e.box_id) else { return false };
        boxes
            .iter()
            .filter(|b| b.node_id >= first && b.node_id < end && b.scroll_root.is_none())
            .any(|b| {
                let paints =
                    b.visual.background.is_some_and(|c| c.a > 0) || b.visual.material.is_some();
                let (x0, y0) = (b.rect.x.0, b.rect.y.0);
                let (x1, y1) = (x0 + b.rect.width.0, y0 + b.rect.height.0);
                paints && x0 < viewport.2 && viewport.0 < x1 && y0 < viewport.3 && viewport.1 < y1
            })
    })
}

/// A full-bleed overlay that only places its content (a closed menu's host) covers no scroll
/// layer — board cycle 2026-11-10: counted by its box, it froze the content's scrolling in
/// every window. The open sidebar sheet does lie over the content.
#[test]
fn test_reject_an_overlay_that_paints_nothing_freezes_no_scrolling() {
    let nxir = common::compile("settings");
    let symbols = common::program_symbols(&nxir);
    let device = FixtureEnv::tablet("portrait");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
    let boxes = common::layout_boxes(&view);
    let content = boxes
        .iter()
        .filter(|b| matches!(b.overflow, nexus_layout_types::Overflow::Scroll(_)))
        .max_by_key(|b| b.rect.width.0 * b.rect.height.0)
        .expect("the content's scroll viewport");
    let r = content.rect;
    let viewport = (r.x.0, r.y.0, r.x.0 + r.width.0, r.y.0 + r.height.0);
    // The bug's shape: a closed host's BOX does lie over the content — counted by its box, it
    // marked the content covered and the band stopped moving.
    let overlays = view.overlays();
    let box_over = overlays.entries().iter().chain(overlays.plain()).any(|e| {
        boxes.iter().find(|b| b.node_id == e.box_id).is_some_and(|b| {
            let (x0, y0) = (b.rect.x.0, b.rect.y.0);
            let (x1, y1) = (x0 + b.rect.width.0, y0 + b.rect.height.0);
            e.box_id > content.node_id
                && x0 < viewport.2
                && viewport.0 < x1
                && y0 < viewport.3
                && viewport.1 < y1
        })
    });
    assert!(box_over, "a closed host's box overlaps the content (what the old rule counted)");
    assert!(!painted_over(&view, &boxes, viewport), "nothing paints over the content while closed");

    common::dispatch(&mut view, &device, &mut NoHost, &symbols, "WindowEvent", "WinLeft", vec![]);
    let boxes = common::layout_boxes(&view);
    assert!(painted_over(&view, &boxes, viewport), "the open sheet lies over the content");
}
