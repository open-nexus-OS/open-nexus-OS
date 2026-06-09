// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Retained scene graph for the production UI runtime.
//! Owns stable node identity, invalidation classes, subtree hashing,
//! and the canonical renderable-primitive vocabulary.
//! All UI frontends (native widgets, design kit, interpreter, AOT) target
//! this single retained scene graph — no alternate rendering vocabulary.
//!
//! OWNERS: @ui
//! STATUS: Phase 1 — types + basic graph operations
//! API_STABILITY: Contract-locked for TASK-0073 through TASK-0120
//! TEST_COVERAGE: Host unit tests in this module

use alloc::vec::Vec;
use animation::{AnimProp, LayerId, SceneUpdate};
use nexus_layout_types::{BoxShadow, Rect, Rgba8};

// ---------------------------------------------------------------------------
// Hash helpers (deterministic, no_std)
// ---------------------------------------------------------------------------

fn hash_seed() -> u64 {
    0x9E3779B97F4A7C15 // golden ratio * 2^64
}

fn hash_u64(seed: u64, value: u64) -> u64 {
    seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(value)
}

fn hash_u32(seed: u64, value: u32) -> u64 {
    hash_u64(seed, value as u64)
}

fn hash_i32(seed: u64, value: i32) -> u64 {
    hash_u64(seed, value as u64)
}

fn hash_f32(seed: u64, value: f32) -> u64 {
    hash_u64(seed, value.to_bits() as u64)
}

fn hash_bool(seed: u64, value: bool) -> u64 {
    hash_u64(seed, value as u64)
}

fn hash_u8s(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for &b in bytes {
        h = hash_u64(h, b as u64);
    }
    h
}

/// Pack Rgba8 into a u32 for hashing (RGBA, big-endian).
fn rgba_hash_value(c: Rgba8) -> u32 {
    ((c.r as u32) << 24) | ((c.g as u32) << 16) | ((c.b as u32) << 8) | (c.a as u32)
}

/// FxPx to u32 for hashing (uses the raw i32 bits; negative values are valid).
fn fxpx_hash_value(v: nexus_layout_types::FxPx) -> u32 {
    v.as_i32() as u32
}

// ---------------------------------------------------------------------------
// Scene node identity
// ---------------------------------------------------------------------------

/// Stable scene-node identifier. Aligned with `animation::LayerId(u64)`.
///
/// Identity survives frame recompositions, layout changes, and tree mutations.
/// Used for dirty tracking, hit-testing, and animation targeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SceneNodeId(pub u64);

impl SceneNodeId {
    pub(crate) const fn new(id: u64) -> Self {
        Self(id)
    }
}

impl From<LayerId> for SceneNodeId {
    fn from(lid: LayerId) -> Self {
        Self(lid.0)
    }
}

impl From<SceneNodeId> for LayerId {
    fn from(id: SceneNodeId) -> Self {
        LayerId(id.0)
    }
}

// ---------------------------------------------------------------------------
// Invalidation classes
// ---------------------------------------------------------------------------

/// What kind of change occurred to a node since the last frame.
///
/// Propagated upward: a `PaintOnly` child makes the parent `PaintOnly`.
/// A `MeasureAndPlace` child makes the parent `MeasureAndPlace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvalidationClass {
    /// Layout changed — recompute measure + place + paint
    MeasureAndPlace,
    /// Position changed — recompute place + paint
    PlaceOnly,
    /// Visual properties changed — recompute paint only
    PaintOnly,
    /// Nothing changed — can skip this subtree entirely
    Clean,
}

impl InvalidationClass {
    /// Merge two invalidation classes: take the more severe.
    pub(crate) fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::MeasureAndPlace, _) | (_, Self::MeasureAndPlace) => Self::MeasureAndPlace,
            (Self::PlaceOnly, _) | (_, Self::PlaceOnly) => Self::PlaceOnly,
            (Self::PaintOnly, _) | (_, Self::PaintOnly) => Self::PaintOnly,
            (Self::Clean, Self::Clean) => Self::Clean,
        }
    }
}

// ---------------------------------------------------------------------------
// Canonical renderable primitives
// ---------------------------------------------------------------------------

/// Canonical renderable primitive vocabulary.
///
/// All frontends (native/runtime widgets, design kit, DSL interpreter, AOT)
/// target these primitives. No alternate rendering vocabulary is allowed.
/// The render backend (gpud) maps each primitive to GPU commands.
#[derive(Debug, Clone)]
pub(crate) enum RenderPrimitive {
    /// Filled rounded rectangle
    Rect {
        width: u32,
        height: u32,
        radius: u32,
        color: Rgba8,
    },
    /// Stroked rounded rectangle border
    StrokeRect {
        width: u32,
        height: u32,
        radius: u32,
        stroke_width: u32,
        color: Rgba8,
    },
    /// Blit from a retained surface (atlas tile, backdrop snapshot, glyph cache)
    Surface {
        surface_handle: u32,
        src_x: u32,
        src_y: u32,
        width: u32,
        height: u32,
    },
    /// Bitmap text run (pre-shaped, pre-rasterized — text shaping is upstream)
    Text {
        /// Content string (borrowed from static assets or entry pool)
        content: &'static str,
        font_scale: u32,
        color: Rgba8,
    },
    /// Backdrop blur filter — samples from a retained backdrop snapshot,
    /// not from live scanout memory
    BackdropFilter {
        blur_radius: u32,
        saturation_percent: u32,
    },
    /// Group container — children rendered into this group with optional shadow
    Group {
        shadow: Option<BoxShadow>,
    },
    /// Hardware or composited cursor
    Cursor {
        hotspot_x: i32,
        hotspot_y: i32,
    },
}

impl RenderPrimitive {
    /// Compute a type-tag for hashing (0-6, one per variant).
    fn tag(&self) -> u64 {
        match self {
            Self::Rect { .. } => 1,
            Self::StrokeRect { .. } => 2,
            Self::Surface { .. } => 3,
            Self::Text { .. } => 4,
            Self::BackdropFilter { .. } => 5,
            Self::Group { .. } => 6,
            Self::Cursor { .. } => 7,
        }
    }

    /// Deterministic hash of the primitive's parameters.
    fn hash(&self, seed: u64) -> u64 {
        let h = hash_u64(seed, self.tag());
        match self {
            Self::Rect { width, height, radius, color } => {
                let h = hash_u32(h, *width);
                let h = hash_u32(h, *height);
                let h = hash_u32(h, *radius);
                hash_u32(h, rgba_hash_value(*color))
            }
            Self::StrokeRect { width, height, radius, stroke_width, color } => {
                let h = hash_u32(h, *width);
                let h = hash_u32(h, *height);
                let h = hash_u32(h, *radius);
                let h = hash_u32(h, *stroke_width);
                hash_u32(h, rgba_hash_value(*color))
            }
            Self::Surface { surface_handle, src_x, src_y, width, height } => {
                let h = hash_u32(h, *surface_handle);
                let h = hash_u32(h, *src_x);
                let h = hash_u32(h, *src_y);
                let h = hash_u32(h, *width);
                hash_u32(h, *height)
            }
            Self::Text { content, font_scale, color } => {
                let h = hash_u8s(h, content.as_bytes());
                let h = hash_u32(h, *font_scale);
                hash_u32(h, rgba_hash_value(*color))
            }
            Self::BackdropFilter { blur_radius, saturation_percent } => {
                let h = hash_u32(h, *blur_radius);
                hash_u32(h, *saturation_percent)
            }
            Self::Group { shadow } => {
                if let Some(s) = shadow {
                    let h = hash_u32(h, fxpx_hash_value(s.offset_x));
                    let h = hash_u32(h, fxpx_hash_value(s.offset_y));
                    let h = hash_u32(h, fxpx_hash_value(s.blur_radius));
                    hash_u32(h, rgba_hash_value(s.color))
                } else {
                    h
                }
            }
            Self::Cursor { hotspot_x, hotspot_y } => {
                let h = hash_i32(h, *hotspot_x);
                hash_i32(h, *hotspot_y)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scene node
// ---------------------------------------------------------------------------

/// A node in the retained scene graph.
///
/// Each node has a stable `id`, a position, visual properties, an optional
/// render primitive, and child nodes. The `subtree_hash` enables O(1) dirty
/// detection: if a node's hash matches the previous frame, the entire subtree
/// can be skipped.
#[derive(Debug, Clone)]
pub(crate) struct SceneNode {
    pub id: SceneNodeId,
    pub parent: Option<SceneNodeId>,
    pub children: Vec<SceneNodeId>,

    // Spatial properties
    pub x: i32,
    pub y: i32,
    pub visible: bool,
    pub opacity: f32,
    pub clip: Option<Rect>,

    // Content
    pub primitive: Option<RenderPrimitive>,

    // Derived state
    pub subtree_hash: u64,
    pub invalidation: InvalidationClass,
}

impl SceneNode {
    pub(crate) fn new(id: SceneNodeId) -> Self {
        Self {
            id,
            parent: None,
            children: Vec::new(),
            x: 0,
            y: 0,
            visible: true,
            opacity: 1.0,
            clip: None,
            primitive: None,
            subtree_hash: 0,
            invalidation: InvalidationClass::MeasureAndPlace,
        }
    }

    /// Compute this node's local hash (properties only, excluding children).
    fn local_hash(&self) -> u64 {
        let h = hash_seed();
        let h = hash_i32(h, self.x);
        let h = hash_i32(h, self.y);
        let h = hash_bool(h, self.visible);
        let h = hash_f32(h, self.opacity);
        let h = if let Some(ref clip) = self.clip {
            let h = hash_u32(h, fxpx_hash_value(clip.x));
            let h = hash_u32(h, fxpx_hash_value(clip.y));
            let h = hash_u32(h, fxpx_hash_value(clip.width));
            hash_u32(h, fxpx_hash_value(clip.height))
       } else {
            h
        };
        if let Some(ref prim) = self.primitive {
            prim.hash(h)
        } else {
            h
        }
    }
}

// ---------------------------------------------------------------------------
// Scene graph
// ---------------------------------------------------------------------------

/// The retained scene graph.
///
/// Holds all scene nodes in a flat `Vec` indexed by `SceneNodeId`.
/// Roots are the top-level nodes (mounted shells, launcher panels,
/// overlays). The graph is retained across frames; only dirty nodes
/// are recomposed.
pub(crate) struct SceneGraph {
    nodes: Vec<Option<SceneNode>>,
    roots: Vec<SceneNodeId>,
    next_id: u64,
}

impl SceneGraph {
    /// Maximum nodes in the graph (bounds Vec growth).
    const MAX_NODES: usize = 256;

    pub(crate) fn new() -> Self {
        // Slot 0 is reserved (invalid/unset).
        let mut nodes = Vec::with_capacity(Self::MAX_NODES);
        nodes.push(None);
        Self { nodes, roots: Vec::new(), next_id: 1 }
    }

    /// Allocate a new node id. Does not insert anything.
    pub(crate) fn next_id(&mut self) -> SceneNodeId {
        let id = SceneNodeId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Insert a node into the graph. Returns its id.
    ///
    /// Panics (in debug) if the id is already in use or exceeds MAX_NODES.
    pub(crate) fn insert(&mut self, mut node: SceneNode) -> SceneNodeId {
        let idx = node.id.0 as usize;
        debug_assert!(idx < Self::MAX_NODES, "SceneGraph: node id exceeds MAX_NODES");
        debug_assert!(node.id.0 > 0, "SceneGraph: node id 0 is reserved");

        // Grow Vec if needed
        while self.nodes.len() <= idx {
            self.nodes.push(None);
        }
        debug_assert!(
            self.nodes[idx].is_none(),
            "SceneGraph: duplicate node id {}",
            node.id.0
        );

        // If no parent, add to roots
        if node.parent.is_none() {
            self.roots.push(node.id);
        }

        // Update parent's children list
        if let Some(parent_id) = node.parent {
            if let Some(parent) = self.find_mut(parent_id) {
                parent.children.push(node.id);
            }
        }

        let id = node.id;
        node.invalidation = InvalidationClass::MeasureAndPlace; // new nodes are dirty
        self.nodes[idx] = Some(node);
        id
    }

    /// Remove a node and all its descendants from the graph.
    pub(crate) fn remove(&mut self, id: SceneNodeId) {
        let idx = id.0 as usize;
        if idx >= self.nodes.len() {
            return;
        }

        // Recursively remove children first
        let children: Vec<SceneNodeId> = self
            .nodes[idx]
            .as_ref()
            .map(|n| n.children.clone())
            .unwrap_or_default();
        for child_id in children {
            self.remove(child_id);
        }

        // Remove from parent's children list
        if let Some(node) = self.nodes[idx].as_ref() {
            if let Some(parent_id) = node.parent {
                if let Some(parent) = self.find_mut(parent_id) {
                    parent.children.retain(|c| *c != id);
                }
            }
        }

        // Remove from roots if present
        self.roots.retain(|r| *r != id);

        // Clear the slot
        self.nodes[idx] = None;
    }

    /// Find a node by id (immutable).
    pub(crate) fn find(&self, id: SceneNodeId) -> Option<&SceneNode> {
        let idx = id.0 as usize;
        self.nodes.get(idx).and_then(|n| n.as_ref())
    }

    /// Find a node by id (mutable).
    pub(crate) fn find_mut(&mut self, id: SceneNodeId) -> Option<&mut SceneNode> {
        let idx = id.0 as usize;
        self.nodes.get_mut(idx).and_then(|n| n.as_mut())
    }

    // ------------------------------------------------------------------
    // Property updates — each marks the appropriate invalidation class
    // ------------------------------------------------------------------

    /// Set node position. Marks `PlaceOnly` (or keeps more severe).
    pub(crate) fn set_position(&mut self, id: SceneNodeId, x: i32, y: i32) {
        if let Some(node) = self.find_mut(id) {
            node.x = x;
            node.y = y;
            node.invalidation = node.invalidation.merge(InvalidationClass::PlaceOnly);
        }
    }

    /// Set node opacity. Marks `PaintOnly`.
    pub(crate) fn set_opacity(&mut self, id: SceneNodeId, opacity: f32) {
        if let Some(node) = self.find_mut(id) {
            node.opacity = opacity;
            node.invalidation = node.invalidation.merge(InvalidationClass::PaintOnly);
        }
    }

    /// Set node visibility. Marks `PaintOnly`.
    pub(crate) fn set_visible(&mut self, id: SceneNodeId, visible: bool) {
        if let Some(node) = self.find_mut(id) {
            node.visible = visible;
            node.invalidation = node.invalidation.merge(InvalidationClass::PaintOnly);
        }
    }

    /// Replace the node's render primitive. Marks `PaintOnly`.
    pub(crate) fn set_primitive(&mut self, id: SceneNodeId, prim: RenderPrimitive) {
        if let Some(node) = self.find_mut(id) {
            node.primitive = Some(prim);
            node.invalidation = node.invalidation.merge(InvalidationClass::PaintOnly);
        }
    }

    /// Apply an animation `SceneUpdate` to the graph.
    ///
    /// Translates `AnimProp` variants to the appropriate node property changes
    /// and invalidation classes.
    pub(crate) fn apply_animation_update(&mut self, update: SceneUpdate) {
        let id = SceneNodeId::from(update.layer_id);
        match update.property {
            AnimProp::Opacity => {
                self.set_opacity(id, update.value);
            }
            AnimProp::TranslateX => {
                if let Some(node) = self.find_mut(id) {
                    node.x = update.value as i32;
                    node.invalidation =
                        node.invalidation.merge(InvalidationClass::PlaceOnly);
                }
            }
            AnimProp::TranslateY => {
                if let Some(node) = self.find_mut(id) {
                    node.y = update.value as i32;
                    node.invalidation =
                        node.invalidation.merge(InvalidationClass::PlaceOnly);
                }
            }
            // ScaleX/Y, ShadowRadius, BlurRadius — deferred to future passes.
            _ => {
                if let Some(node) = self.find_mut(id) {
                    node.invalidation =
                        node.invalidation.merge(InvalidationClass::PaintOnly);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Dirty set computation
    // ------------------------------------------------------------------

    /// Compute subtree hashes and propagate invalidation upward.
    ///
    /// Returns the set of dirty node ids (nodes whose `invalidation != Clean`
    /// after propagation). Callers should regenerate commands for these nodes.
    ///
    /// Algorithm:
    /// 1. Bottom-up: for each dirty node, recompute `subtree_hash` from
    ///    `local_hash()` + children's `subtree_hash` values.
    /// 2. Propagate: if a child is dirty (!= Clean), mark the parent at least
    ///    as dirty as the child's class.
    /// 3. Top-down: any parent that was propagated into also propagates to
    ///    siblings (a dirty parent means all children may need recomposition).
    /// 4. Collect all nodes with `invalidation != Clean`.
    pub(crate) fn compute_dirty_set(&mut self) -> Vec<SceneNodeId> {
        // Phase 1: bottom-up hash + invalidation propagation.
        // Avoid borrow conflicts by collecting child data first, then applying.
        let mut changed = true;
        while changed {
            changed = false;

            // Collect updates: (node_id, new_hash, merged_invalidation)
            let mut updates: alloc::vec::Vec<(SceneNodeId, u64, InvalidationClass)> =
                alloc::vec::Vec::new();

            for idx in 1..self.nodes.len() {
                let node = match self.nodes[idx].as_ref() {
                    Some(n) => n,
                    None => continue,
                };
                if node.invalidation == InvalidationClass::Clean {
                    continue;
                }

                // Compute new hash from local + children (use raw lookups)
                let mut hash = node.local_hash();
                for &child_id in &node.children {
                    let cidx = child_id.0 as usize;
                    if let Some(Some(child)) = self.nodes.get(cidx) {
                        hash = hash_u64(hash, child.subtree_hash);
                    }
                }

                // Propagate child dirtiness
                let mut merged = node.invalidation;
                for &child_id in &node.children {
                    let cidx = child_id.0 as usize;
                    if let Some(Some(child)) = self.nodes.get(cidx) {
                        if child.invalidation != InvalidationClass::Clean {
                            merged = merged.merge(child.invalidation);
                        }
                    }
                }

                if hash != node.subtree_hash || merged != node.invalidation {
                    updates.push((node.id, hash, merged));
                }
            }

            // Apply updates and propagate to parents
            for (id, hash, inval) in &updates {
                if let Some(node) = self.find_mut(*id) {
                    node.subtree_hash = *hash;
                    node.invalidation = *inval;
                    // Mark parent as at least this dirty for the next iteration
                    if let Some(parent_id) = node.parent {
                        if let Some(parent) = self.find_mut(parent_id) {
                            let new_inval = parent.invalidation.merge(*inval);
                            if new_inval != parent.invalidation {
                                parent.invalidation = new_inval;
                            }
                        }
                    }
                    changed = true;
                }
            }
        }

        // Phase 2: collect dirty nodes
        let mut dirty = Vec::new();
        for idx in 1..self.nodes.len() {
            if let Some(node) = self.nodes[idx].as_ref() {
                if node.invalidation != InvalidationClass::Clean {
                    dirty.push(node.id);
                }
            }
        }
        dirty
    }

    /// Mark all nodes clean (call after frame submission).
    pub(crate) fn mark_all_clean(&mut self) {
        for slot in &mut self.nodes {
            if let Some(node) = slot {
                node.invalidation = InvalidationClass::Clean;
            }
        }
    }

    /// Number of live nodes.
    pub(crate) fn node_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::Rgba8;

    fn make_graph() -> SceneGraph {
        SceneGraph::new()
    }

    fn rect_prim(w: u32, h: u32) -> RenderPrimitive {
        RenderPrimitive::Rect {
            width: w,
            height: h,
            radius: 4,
            color: Rgba8::new(255, 0, 0, 255),
        }
    }

    fn make_node(graph: &mut SceneGraph, parent: Option<SceneNodeId>) -> SceneNodeId {
        let id = graph.next_id();
        let mut node = SceneNode::new(id);
        node.parent = parent;
        graph.insert(node)
    }

    #[test]
    fn new_graph_is_empty() {
        let g = make_graph();
        assert_eq!(g.node_count(), 0);
        assert!(g.roots.is_empty());
    }

    #[test]
    fn insert_single_node() {
        let mut g = make_graph();
        let id = make_node(&mut g, None);
        assert_eq!(g.node_count(), 1);
        assert_eq!(g.roots, vec![id]);
        assert!(g.find(id).is_some());
    }

    #[test]
    fn insert_parent_child_tree() {
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let child = make_node(&mut g, Some(root));

        assert_eq!(g.node_count(), 2);
        assert_eq!(g.roots, vec![root]);
        let root_node = g.find(root).unwrap();
        assert_eq!(root_node.children, vec![child]);
        let child_node = g.find(child).unwrap();
        assert_eq!(child_node.parent, Some(root));
    }

    #[test]
    fn remove_node_removes_children() {
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let _child = make_node(&mut g, Some(root));

        g.remove(root);
        assert_eq!(g.node_count(), 0);
        assert!(g.roots.is_empty());
    }

    #[test]
    fn set_position_marks_place_only() {
        let mut g = make_graph();
        let id = make_node(&mut g, None);

        // Initially MeasureAndPlace (new node)
        assert_eq!(g.find(id).unwrap().invalidation, InvalidationClass::MeasureAndPlace);

        // Mark clean, then move
        g.mark_all_clean();
        g.set_position(id, 10, 20);
        assert_eq!(g.find(id).unwrap().invalidation, InvalidationClass::PlaceOnly);
    }

    #[test]
    fn set_opacity_marks_paint_only() {
        let mut g = make_graph();
        let id = make_node(&mut g, None);
        g.mark_all_clean();
        g.set_opacity(id, 0.5);
        assert_eq!(g.find(id).unwrap().invalidation, InvalidationClass::PaintOnly);
    }

    #[test]
    fn dirty_child_propagates_to_parent() {
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let child = make_node(&mut g, Some(root));

        g.mark_all_clean();
        assert_eq!(g.find(root).unwrap().invalidation, InvalidationClass::Clean);
        assert_eq!(g.find(child).unwrap().invalidation, InvalidationClass::Clean);

        g.set_opacity(child, 0.5);
        let dirty = g.compute_dirty_set();
        assert!(dirty.contains(&root), "parent should be dirty");
        assert!(dirty.contains(&child), "child should be dirty");
    }

    #[test]
    fn subtree_hash_changes_on_primitive_change() {
        let mut g = make_graph();
        let id = make_node(&mut g, None);
        g.mark_all_clean();
        g.compute_dirty_set();
        let hash_before = g.find(id).unwrap().subtree_hash;

        g.set_primitive(id, rect_prim(100, 200));
        g.compute_dirty_set();
        let hash_after = g.find(id).unwrap().subtree_hash;

        assert_ne!(hash_before, hash_after, "hash should change when primitive changes");
    }

    #[test]
    fn subtree_hash_stable_when_clean() {
        let mut g = make_graph();
        let id = make_node(&mut g, None);
        g.set_primitive(id, rect_prim(50, 50));
        g.mark_all_clean();
        g.compute_dirty_set();
        let hash1 = g.find(id).unwrap().subtree_hash;

        g.mark_all_clean();
        g.compute_dirty_set();
        let hash2 = g.find(id).unwrap().subtree_hash;

        assert_eq!(hash1, hash2, "hash should be stable across clean frames");
    }

    #[test]
    fn child_hash_included_in_parent_hash() {
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let child = make_node(&mut g, Some(root));
        g.set_primitive(child, rect_prim(10, 10));
        g.mark_all_clean();
        g.compute_dirty_set();
        let hash1 = g.find(root).unwrap().subtree_hash;

        g.set_primitive(child, rect_prim(20, 20));
        g.compute_dirty_set();
        let hash2 = g.find(root).unwrap().subtree_hash;

        assert_ne!(hash1, hash2, "parent hash should change when child changes");
    }

    #[test]
    fn animation_update_sets_position() {
        let mut g = make_graph();
        let id = make_node(&mut g, None);
        g.mark_all_clean();

        g.apply_animation_update(SceneUpdate {
            layer_id: LayerId(id.0),
            property: AnimProp::TranslateX,
            progress: 0.0,
            value: 42.0,
        });

        let node = g.find(id).unwrap();
        assert_eq!(node.x, 42);
        assert_eq!(node.invalidation, InvalidationClass::PlaceOnly);
    }

    #[test]
    fn animation_update_sets_opacity() {
        let mut g = make_graph();
        let id = make_node(&mut g, None);
        g.mark_all_clean();

        g.apply_animation_update(SceneUpdate {
            layer_id: LayerId(id.0),
            property: AnimProp::Opacity,
            progress: 0.0,
            value: 0.3,
        });

        let node = g.find(id).unwrap();
        assert_eq!(node.opacity, 0.3);
        assert_eq!(node.invalidation, InvalidationClass::PaintOnly);
    }

    #[test]
    fn mark_all_clean_resets_invalidation() {
        let mut g = make_graph();
        let id = make_node(&mut g, None);
        g.set_opacity(id, 0.5);
        assert_ne!(g.find(id).unwrap().invalidation, InvalidationClass::Clean);

        g.mark_all_clean();
        assert_eq!(g.find(id).unwrap().invalidation, InvalidationClass::Clean);
    }

    #[test]
    fn multiple_roots() {
        let mut g = make_graph();
        let a = make_node(&mut g, None);
        let b = make_node(&mut g, None);
        assert_eq!(g.roots.len(), 2);
        assert!(g.roots.contains(&a));
        assert!(g.roots.contains(&b));
    }

    #[test]
    fn compute_dirty_set_empty_when_all_clean() {
        let mut g = make_graph();
        let _root = make_node(&mut g, None);
        g.mark_all_clean();
        let dirty = g.compute_dirty_set();
        assert!(dirty.is_empty(), "no nodes should be dirty after mark_all_clean");
    }

    #[test]
    fn invalidation_merge_priority() {
        assert_eq!(
            InvalidationClass::PaintOnly.merge(InvalidationClass::PlaceOnly),
            InvalidationClass::PlaceOnly
        );
        assert_eq!(
            InvalidationClass::PlaceOnly.merge(InvalidationClass::MeasureAndPlace),
            InvalidationClass::MeasureAndPlace
        );
        assert_eq!(
            InvalidationClass::Clean.merge(InvalidationClass::Clean),
            InvalidationClass::Clean
        );
    }

    #[test]
    fn render_primitive_tag_uniqueness() {
        // Each variant must have a unique tag
        let tags: Vec<u64> = vec![
            RenderPrimitive::Rect { width: 0, height: 0, radius: 0, color: Rgba8::new(0, 0, 0, 0) }.tag(),
            RenderPrimitive::StrokeRect { width: 0, height: 0, radius: 0, stroke_width: 0, color: Rgba8::new(0, 0, 0, 0) }.tag(),
            RenderPrimitive::Surface { surface_handle: 0, src_x: 0, src_y: 0, width: 0, height: 0 }.tag(),
            RenderPrimitive::Text { content: "", font_scale: 0, color: Rgba8::new(0, 0, 0, 0) }.tag(),
            RenderPrimitive::BackdropFilter { blur_radius: 0, saturation_percent: 0 }.tag(),
            RenderPrimitive::Group { shadow: None }.tag(),
            RenderPrimitive::Cursor { hotspot_x: 0, hotspot_y: 0 }.tag(),
        ];
        let mut seen = alloc::vec::Vec::new();
        for t in &tags {
            assert!(!seen.contains(t), "duplicate tag {t}");
            seen.push(*t);
        }
        assert_eq!(seen.len(), 7);
    }

    #[test]
    fn layer_id_scene_node_id_roundtrip() {
        let lid = LayerId(12345);
        let sid: SceneNodeId = lid.into();
        let lid2: LayerId = sid.into();
        assert_eq!(lid, lid2);
    }
}