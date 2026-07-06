// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Interaction routing: handler regions + hit-testing.
//!
//! Handlers are collected during emission with their **path in the final
//! scene tree** (child indices from the root). Because the layout engine
//! assigns `LayoutBox::node_id` in pre-order (one box per node, counter
//! starts at 1), a path converts to the box id by a counting walk — no ids
//! smuggled through `&'static str`, no parallel bookkeeping in the engine.
//!
//! Dispatch payloads are evaluated **at emit time**: a collection item's
//! handler captures that item's values (the only correct reading once the
//! loop binding is gone). Handlers whose payload reads state therefore
//! record a Paint-class dependency — any change re-emits and re-captures.

use crate::store::Value;
use alloc::vec::Vec;
use nexus_layout_types::{FxPx, LayoutNode};

/// What a triggered handler does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerAction {
    /// Dispatch a store event (payload captured at emit time).
    Dispatch { event: u32, case: u32, payload: Vec<Value> },
    /// Navigate to a route path (evaluated at emit time).
    Navigate { path: alloc::string::String },
    /// Two-way binding write target: (store index, field symbol path).
    /// The interaction supplies the value (Tap on a bound Toggle flips the
    /// Bool; text input writes the new text).
    Bind { store: u32, path: Vec<u32> },
}

/// One interactive region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerEntry {
    /// Child-index path from the scene root to the handler's node.
    pub path: Vec<u32>,
    /// Interaction trigger symbol id (`Tap`, `Change`, …).
    pub trigger: u32,
    pub action: HandlerAction,
}

/// Pre-order box id (1-based, matches `LayoutBox::node_id`) for a path.
#[must_use]
pub fn path_to_box_id(scene: &LayoutNode, path: &[u32]) -> Option<usize> {
    fn count_nodes(node: &LayoutNode) -> usize {
        match node {
            LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => {
                1 + children.iter().map(count_nodes).sum::<usize>()
            }
            LayoutNode::Spacer(_) | LayoutNode::Text(_, _) | LayoutNode::TextInput(_, _) => 1,
        }
    }
    let mut id = 1usize; // the root
    let mut node = scene;
    for &index in path {
        let children = match node {
            LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => children,
            _ => return None,
        };
        let index = index as usize;
        if index >= children.len() {
            return None;
        }
        // Skip the boxes of all earlier siblings + ourselves (the parent).
        id += 1 + children[..index].iter().map(count_nodes).sum::<usize>();
        node = &children[index];
    }
    Some(id)
}

/// Finds the innermost handler for `trigger_sym` whose box contains (x, y).
///
/// `boxes` is the flat `LayoutResult::boxes`; deeper nodes have larger
/// pre-order ids, so the max matching id wins (innermost target).
#[must_use]
pub fn hit<'h>(
    handlers: &'h [(usize, HandlerEntry)],
    boxes: &[nexus_layout::LayoutBox],
    trigger_sym: u32,
    x: FxPx,
    y: FxPx,
) -> Option<&'h HandlerEntry> {
    let mut best: Option<(usize, &HandlerEntry)> = None;
    for (box_id, entry) in handlers {
        if entry.trigger != trigger_sym {
            continue;
        }
        let Some(layout_box) = boxes.iter().find(|b| b.node_id == *box_id) else {
            continue;
        };
        let rect = layout_box.rect;
        let inside = x >= rect.x
            && y >= rect.y
            && x < rect.x + rect.width
            && y < rect.y + rect.height;
        if inside && best.map_or(true, |(id, _)| *box_id > id) {
            best = Some((*box_id, entry));
        }
    }
    best.map(|(_, entry)| entry)
}
