// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Retained scene graph for the production UI runtime.
//! Owns stable node identity, invalidation classes, subtree hashing,
//! and the canonical renderable-primitive vocabulary.
//! All UI frontends (native widgets, design kit, interpreter, AOT) target
//! this single retained scene graph — no alternate rendering vocabulary.
//!
//! PRODUCTION INVARIANTS (enforced by tests in this module):
//! - **Zero per-frame heap allocation**: the OS bump allocator never frees,
//!   so the frame loop only uses persistent, capacity-retaining buffers
//!   (`compute_dirty_set_into`, `render_order_into`, internal dirty list and
//!   text-tile scratch). Allocation happens at mount/insert time only.
//! - **O(dirty) invalidation**: mutations enqueue nodes in an explicit dirty
//!   list; `compute_dirty_set_into` touches only dirty nodes + their ancestor
//!   chains (`last_hash_recomputes` is the observable bound). Clean subtrees
//!   are never visited or re-hashed.
//! - **No per-node heap**: children are intrusive sibling links
//!   (`first_child`/`last_child`/`next_sibling`), preserving insertion order
//!   (= back-to-front z-order) with O(1) append.
//! - **Explicit animation targeting**: animation updates address nodes via
//!   `apply_animation_update_to` with a target resolved by the layer→node
//!   registry (`SystemUiShell::animation_target`) — id-punning LayerId as
//!   SceneNodeId is forbidden (it silently animated the root/wallpaper).
//!
//! OWNERS: @ui
//! STATUS: Phase 2 — production-hardened graph core (bump-safe, O(dirty))
//! API_STABILITY: Contract-locked for TASK-0073 through TASK-0120
//! TEST_COVERAGE: Host unit tests in this module + tests/ui_v5b_host

use alloc::vec::Vec;
use animation::{AnimProp, LayerId, SceneUpdate};
use nexus_gfx::command::buffer::RgbaColor;
use nexus_gfx::command::render_encoder::RenderCommandEncoder;
use nexus_gfx::core::error::GfxError;
use nexus_gfx::core::types::TileRect;
use nexus_layout_types::{BoxShadow, Rect, Rgba8};

/// Clamp a tile rect to the render extent `(w, h)`.
///
/// Returns `None` if the rect starts outside the extent or collapses to zero
/// area after clamping. The GPU command validator rejects any rect that
/// overruns the framebuffer, so emitting an unclamped edge-touching rect would
/// abort the whole frame; clamping preserves the on-screen portion instead.
fn clamp_tile_to_extent(rect: TileRect, w: u32, h: u32) -> Option<TileRect> {
    if rect.x >= w || rect.y >= h {
        return None;
    }
    let width = rect.width.min(w - rect.x);
    let height = rect.height.min(h - rect.y);
    if width == 0 || height == 0 {
        return None;
    }
    Some(TileRect {
        x: rect.x,
        y: rect.y,
        width,
        height,
    })
}

/// Intersection of two tile rects, or `None` if they do not overlap.
fn intersect_tile(a: TileRect, b: TileRect) -> Option<TileRect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(TileRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 })
}

/// Bounding union of two tile rects.
fn union_tile(a: TileRect, b: TileRect) -> TileRect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    TileRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
}

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
pub struct SceneNodeId(pub u64);

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
pub enum InvalidationClass {
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
    pub fn merge(self, other: Self) -> Self {
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
pub enum RenderPrimitive {
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
    Group { shadow: Option<BoxShadow> },
    /// Hardware or composited cursor
    Cursor { hotspot_x: i32, hotspot_y: i32 },
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
            Self::Rect {
                width,
                height,
                radius,
                color,
            } => {
                let h = hash_u32(h, *width);
                let h = hash_u32(h, *height);
                let h = hash_u32(h, *radius);
                hash_u32(h, rgba_hash_value(*color))
            }
            Self::StrokeRect {
                width,
                height,
                radius,
                stroke_width,
                color,
            } => {
                let h = hash_u32(h, *width);
                let h = hash_u32(h, *height);
                let h = hash_u32(h, *radius);
                let h = hash_u32(h, *stroke_width);
                hash_u32(h, rgba_hash_value(*color))
            }
            Self::Surface {
                surface_handle,
                src_x,
                src_y,
                width,
                height,
            } => {
                let h = hash_u32(h, *surface_handle);
                let h = hash_u32(h, *src_x);
                let h = hash_u32(h, *src_y);
                let h = hash_u32(h, *width);
                hash_u32(h, *height)
            }
            Self::Text {
                content,
                font_scale,
                color,
            } => {
                let h = hash_u8s(h, content.as_bytes());
                let h = hash_u32(h, *font_scale);
                hash_u32(h, rgba_hash_value(*color))
            }
            Self::BackdropFilter {
                blur_radius,
                saturation_percent,
            } => {
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
            Self::Cursor {
                hotspot_x,
                hotspot_y,
            } => {
                let h = hash_i32(h, *hotspot_x);
                hash_i32(h, *hotspot_y)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scene node
// ---------------------------------------------------------------------------

/// Sentinel id meaning "no node" in the intrusive sibling links (slot 0 is
/// reserved, so id 0 can never name a live node).
const NIL: SceneNodeId = SceneNodeId(0);

/// A node in the retained scene graph.
///
/// Each node has a stable `id`, a position, visual properties, an optional
/// render primitive, and child nodes. The `subtree_hash` enables O(1) dirty
/// detection: if a node's hash matches the previous frame, the entire subtree
/// can be skipped.
///
/// Children are stored as **intrusive sibling links** (`first_child` /
/// `last_child` / `next_sibling`, sentinel = id 0) instead of a per-node
/// `Vec`: under the OS bump allocator (which never frees) a `Vec` per node
/// both leaks on growth and costs one heap allocation per node. The links
/// preserve insertion order (= back-to-front z-order) with O(1) append.
#[derive(Debug, Clone)]
pub struct SceneNode {
    pub id: SceneNodeId,
    pub parent: Option<SceneNodeId>,
    first_child: SceneNodeId,
    last_child: SceneNodeId,
    next_sibling: SceneNodeId,

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
    /// True while the node sits in the graph's dirty list (dedup flag).
    in_dirty_list: bool,
}

impl SceneNode {
    pub fn new(id: SceneNodeId) -> Self {
        Self {
            id,
            parent: None,
            first_child: NIL,
            last_child: NIL,
            next_sibling: NIL,
            x: 0,
            y: 0,
            visible: true,
            opacity: 1.0,
            clip: None,
            primitive: None,
            subtree_hash: 0,
            invalidation: InvalidationClass::MeasureAndPlace,
            in_dirty_list: false,
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
pub struct SceneGraph {
    nodes: Vec<Option<SceneNode>>,
    roots: Vec<SceneNodeId>,
    next_id: u64,
    /// Nodes currently marked dirty (deduped via `SceneNode::in_dirty_list`).
    /// Persistent across frames: `Vec::clear` keeps capacity, so steady-state
    /// frames mark/compute/clean without any heap allocation — mandatory under
    /// the OS bump allocator, which never frees.
    dirty_list: Vec<SceneNodeId>,
    /// Scratch for bitmap-text tile emission in `generate_commands_into`
    /// (reused every frame for the same reason).
    text_tiles: Vec<TileRect>,
    /// Diagnostic: how many subtree hashes the last `compute_dirty_set_into`
    /// recomputed. An O(dirty) invariant check — a single-leaf change must
    /// recompute ~depth hashes, not the whole graph.
    hash_recomputes: u32,
}

impl SceneGraph {
    /// Maximum nodes in the graph (bounds Vec growth).
    /// Raised to 2048 for virtual list + chat mockup (TASK-0063 Phase 1).
    /// Decision: if bump-allocator pressure on OS is too high, reduce to 1024.
    const MAX_NODES: usize = 2048;
    /// Bound on upward-propagation / hash-fixpoint passes — a sanity guard far
    /// above any real tree depth (shell tree is 4 levels deep).
    const MAX_DEPTH: usize = 16;

    pub fn new() -> Self {
        // Slot 0 is reserved (invalid/unset).
        let mut nodes = Vec::with_capacity(Self::MAX_NODES);
        nodes.push(None);
        Self {
            nodes,
            roots: Vec::new(),
            next_id: 1,
            dirty_list: Vec::with_capacity(256),
            text_tiles: Vec::with_capacity(256),
            hash_recomputes: 0,
        }
    }

    /// Allocate a new node id. Does not insert anything.
    pub fn next_id(&mut self) -> SceneNodeId {
        let id = SceneNodeId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Insert a node into the graph. Returns its id.
    ///
    /// Panics (in debug) if the id is already in use or exceeds MAX_NODES.
    pub fn insert(&mut self, mut node: SceneNode) -> SceneNodeId {
        let idx = node.id.0 as usize;
        debug_assert!(
            idx < Self::MAX_NODES,
            "SceneGraph: node id exceeds MAX_NODES"
        );
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

        // Append to the parent's sibling chain (O(1) via last_child;
        // insertion order = back-to-front z-order).
        if let Some(parent_id) = node.parent {
            let prev_last = match self.find(parent_id) {
                Some(parent) => parent.last_child,
                None => NIL,
            };
            if let Some(parent) = self.find_mut(parent_id) {
                if parent.first_child == NIL {
                    parent.first_child = node.id;
                }
                parent.last_child = node.id;
            }
            if prev_last != NIL {
                if let Some(prev) = self.find_mut(prev_last) {
                    prev.next_sibling = node.id;
                }
            }
        }

        let id = node.id;
        node.first_child = NIL;
        node.last_child = NIL;
        node.next_sibling = NIL;
        node.invalidation = InvalidationClass::MeasureAndPlace; // new nodes are dirty
        node.in_dirty_list = true;
        self.nodes[idx] = Some(node);
        self.dirty_list.push(id);
        id
    }

    /// Remove a node and all its descendants from the graph.
    pub fn remove(&mut self, id: SceneNodeId) {
        let idx = id.0 as usize;
        if idx >= self.nodes.len() || self.nodes[idx].is_none() {
            return;
        }

        // Unlink from the parent's sibling chain first (descendants keep their
        // links to each other; they are dropped wholesale below).
        let (parent, _first_child) = match self.nodes[idx].as_ref() {
            Some(n) => (n.parent, n.first_child),
            None => return,
        };
        if let Some(parent_id) = parent {
            self.unlink_child(parent_id, id);
            // Structural change: re-render the parent subtree.
            self.mark_dirty(parent_id, InvalidationClass::MeasureAndPlace);
        } else {
            self.roots.retain(|r| *r != id);
        }

        // Drop the subtree depth-first via the sibling links (recursion depth
        // = tree depth, bounded and shallow; no per-node allocation).
        self.drop_subtree(id);
    }

    fn drop_subtree(&mut self, id: SceneNodeId) {
        let idx = id.0 as usize;
        let first = match self.nodes.get(idx).and_then(|n| n.as_ref()) {
            Some(n) => n.first_child,
            None => return,
        };
        let mut child = first;
        while child != NIL {
            let next = self
                .nodes
                .get(child.0 as usize)
                .and_then(|n| n.as_ref())
                .map(|n| n.next_sibling)
                .unwrap_or(NIL);
            self.drop_subtree(child);
            child = next;
        }
        self.nodes[idx] = None;
    }

    /// Remove `child` from `parent_id`'s sibling chain (order-preserving).
    fn unlink_child(&mut self, parent_id: SceneNodeId, child: SceneNodeId) {
        let first = match self.find(parent_id) {
            Some(p) => p.first_child,
            None => return,
        };
        if first == child {
            let next = self.find(child).map(|n| n.next_sibling).unwrap_or(NIL);
            if let Some(p) = self.find_mut(parent_id) {
                p.first_child = next;
                if p.last_child == child {
                    p.last_child = NIL;
                }
            }
            return;
        }
        // Walk the chain to find the predecessor.
        let mut prev = first;
        while prev != NIL {
            let next = self.find(prev).map(|n| n.next_sibling).unwrap_or(NIL);
            if next == child {
                let after = self.find(child).map(|n| n.next_sibling).unwrap_or(NIL);
                if let Some(p) = self.find_mut(prev) {
                    p.next_sibling = after;
                }
                if let Some(p) = self.find_mut(parent_id) {
                    if p.last_child == child {
                        p.last_child = prev;
                    }
                }
                return;
            }
            prev = next;
        }
    }

    /// Find a node by id (immutable).
    pub fn find(&self, id: SceneNodeId) -> Option<&SceneNode> {
        let idx = id.0 as usize;
        self.nodes.get(idx).and_then(|n| n.as_ref())
    }

    /// Find a node by id (mutable).
    pub fn find_mut(&mut self, id: SceneNodeId) -> Option<&mut SceneNode> {
        let idx = id.0 as usize;
        self.nodes.get_mut(idx).and_then(|n| n.as_mut())
    }

    /// Iterate a node's children in insertion (z) order — allocation-free.
    pub fn children(&self, id: SceneNodeId) -> ChildIter<'_> {
        let first = self.find(id).map(|n| n.first_child).unwrap_or(NIL);
        ChildIter {
            graph: self,
            next: first,
        }
    }

    /// Number of children of `id` (O(children), allocation-free).
    pub fn child_count(&self, id: SceneNodeId) -> usize {
        self.children(id).count()
    }

    /// Centralized dirty marking: merges the invalidation class and enqueues
    /// the node in the dirty list exactly once. Every mutating API routes
    /// through here so the dirty list is the single source of "what changed".
    fn mark_dirty(&mut self, id: SceneNodeId, class: InvalidationClass) {
        let mut enqueue = false;
        if let Some(node) = self.find_mut(id) {
            node.invalidation = node.invalidation.merge(class);
            if !node.in_dirty_list {
                node.in_dirty_list = true;
                enqueue = true;
            }
        }
        if enqueue {
            self.dirty_list.push(id);
        }
    }

    // ------------------------------------------------------------------
    // Property updates — each marks the appropriate invalidation class
    // ------------------------------------------------------------------

    /// Set node position. Marks `PlaceOnly` (or keeps more severe).
    pub fn set_position(&mut self, id: SceneNodeId, x: i32, y: i32) {
        if let Some(node) = self.find_mut(id) {
            node.x = x;
            node.y = y;
        }
        self.mark_dirty(id, InvalidationClass::PlaceOnly);
    }

    /// Set node opacity. Marks `PaintOnly`.
    pub fn set_opacity(&mut self, id: SceneNodeId, opacity: f32) {
        if let Some(node) = self.find_mut(id) {
            node.opacity = opacity;
        }
        self.mark_dirty(id, InvalidationClass::PaintOnly);
    }

    /// Replace the node's render primitive. Marks `PaintOnly`.
    pub fn set_primitive(&mut self, id: SceneNodeId, prim: RenderPrimitive) {
        if let Some(node) = self.find_mut(id) {
            node.primitive = Some(prim);
        }
        self.mark_dirty(id, InvalidationClass::PaintOnly);
    }

    /// Apply an animation `SceneUpdate` to an **explicit** target node.
    ///
    /// The target must be resolved by the owner of the layer→node mapping
    /// (`SystemUiShell::animation_target`). The historical id-punning
    /// `SceneNodeId::from(LayerId)` silently hit unrelated nodes (LayerId(1)
    /// was the root, LayerId(62) did not exist) — do not reintroduce it.
    pub fn apply_animation_update_to(&mut self, id: SceneNodeId, update: SceneUpdate) {
        match update.property {
            AnimProp::Opacity => {
                self.set_opacity(id, update.value);
            }
            AnimProp::TranslateX => {
                if let Some(node) = self.find_mut(id) {
                    node.x = update.value as i32;
                }
                self.mark_dirty(id, InvalidationClass::PlaceOnly);
            }
            AnimProp::TranslateY => {
                if let Some(node) = self.find_mut(id) {
                    node.y = update.value as i32;
                }
                self.mark_dirty(id, InvalidationClass::PlaceOnly);
            }
            // ScaleX/Y, ShadowRadius, BlurRadius — deferred to future passes.
            _ => {
                self.mark_dirty(id, InvalidationClass::PaintOnly);
            }
        }
    }

    // ------------------------------------------------------------------
    // Dirty set computation
    // ------------------------------------------------------------------

    /// Compute the dirty set — O(dirty), allocation-free in steady state.
    ///
    /// Only nodes that were explicitly mutated since the last
    /// `mark_all_clean` (tracked in the internal dirty list) and their
    /// ancestor chains are touched; clean subtrees are never visited or
    /// re-hashed. The result (dirty nodes + their ancestors, i.e. every node
    /// whose `invalidation != Clean` after upward propagation) is written
    /// into `out`, which callers keep alive across frames so its capacity is
    /// reused (`Vec::clear` retains the allocation).
    ///
    /// Algorithm:
    /// 1. Upward propagation: each dirty node merges its invalidation class
    ///    into its ancestors; newly-dirtied ancestors join the dirty list.
    /// 2. Hash refresh: bounded fixpoint over the dirty list recomputing
    ///    `subtree_hash` (= local hash ⊕ children hashes) until stable —
    ///    converges in ≤ tree-depth passes; only listed nodes are hashed.
    pub fn compute_dirty_set_into(&mut self, out: &mut Vec<SceneNodeId>) {
        out.clear();
        self.hash_recomputes = 0;
        if self.dirty_list.is_empty() {
            return;
        }

        // Phase 1: propagate invalidation to ancestors. Index loop because the
        // list grows while iterating (newly-dirtied ancestors are appended).
        let mut i = 0;
        while i < self.dirty_list.len() {
            let id = self.dirty_list[i];
            i += 1;
            let (mut parent, class) = match self.find(id) {
                Some(n) => (n.parent, n.invalidation),
                None => continue, // removed after being marked
            };
            let mut hops = 0;
            while let Some(pid) = parent {
                if hops >= Self::MAX_DEPTH {
                    break;
                }
                hops += 1;
                let next = match self.find(pid) {
                    Some(p) => p.parent,
                    None => break,
                };
                self.mark_dirty(pid, class);
                parent = next;
            }
        }

        // Phase 2: refresh subtree hashes bottom-up. A pass recomputes every
        // listed node's hash from its children's current hashes; after at most
        // tree-depth passes the values are stable.
        for _ in 0..Self::MAX_DEPTH {
            let mut changed = false;
            for i in 0..self.dirty_list.len() {
                let id = self.dirty_list[i];
                let idx = id.0 as usize;
                let (new_hash, old_hash) = {
                    let node = match self.nodes.get(idx).and_then(|n| n.as_ref()) {
                        Some(n) => n,
                        None => continue,
                    };
                    let mut hash = node.local_hash();
                    let mut child = node.first_child;
                    while child != NIL {
                        let cidx = child.0 as usize;
                        match self.nodes.get(cidx).and_then(|n| n.as_ref()) {
                            Some(c) => {
                                hash = hash_u64(hash, c.subtree_hash);
                                child = c.next_sibling;
                            }
                            None => break,
                        }
                    }
                    (hash, node.subtree_hash)
                };
                self.hash_recomputes = self.hash_recomputes.saturating_add(1);
                if new_hash != old_hash {
                    if let Some(node) = self.nodes[idx].as_mut() {
                        node.subtree_hash = new_hash;
                    }
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Phase 3: emit the dirty set (skip ids removed in the meantime).
        for &id in &self.dirty_list {
            if self.find(id).is_some() {
                out.push(id);
            }
        }
    }

    /// Test/diagnostic convenience — allocating variant of
    /// `compute_dirty_set_into`. Production frame paths must use the `_into`
    /// form with a persistent buffer (bump allocator: per-frame allocs leak).
    pub fn compute_dirty_set(&mut self) -> Vec<SceneNodeId> {
        let mut out = Vec::new();
        self.compute_dirty_set_into(&mut out);
        out
    }

    /// Diagnostic: subtree hashes recomputed by the last dirty-set pass.
    pub fn last_hash_recomputes(&self) -> u32 {
        self.hash_recomputes
    }

    /// Mark all nodes clean (call after frame submission). O(dirty): only the
    /// nodes in the dirty list can be non-Clean, so only they are visited.
    pub fn mark_all_clean(&mut self) {
        for i in 0..self.dirty_list.len() {
            let id = self.dirty_list[i];
            if let Some(node) = self.find_mut(id) {
                node.invalidation = InvalidationClass::Clean;
                node.in_dirty_list = false;
            }
        }
        self.dirty_list.clear();
    }

    /// Number of live nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }

    /// All live node ids in ascending-index (= insertion = back-to-front z)
    /// order, for a complete frame repaint — written into a caller-persistent
    /// buffer (allocation-free in steady state).
    pub fn render_order_into(&self, out: &mut Vec<SceneNodeId>) {
        out.clear();
        for (idx, slot) in self.nodes.iter().enumerate() {
            if slot.is_some() {
                out.push(SceneNodeId(idx as u64));
            }
        }
    }

    // ------------------------------------------------------------------
    // Batch operations + recycling (virtual list / large collections)
    // ------------------------------------------------------------------

    /// Insert multiple nodes at once, allocating ids sequentially.
    /// Returns the list of assigned ids in insertion order.
    pub fn batch_insert(&mut self, nodes: Vec<SceneNode>) -> Vec<SceneNodeId> {
        let mut ids = Vec::with_capacity(nodes.len());
        for mut node in nodes {
            node.id = self.next_id();
            ids.push(node.id);
            self.insert(node);
        }
        ids
    }

    /// Reset a recycled node for reuse with a new primitive and position.
    /// Marks `MeasureAndPlace` so the node is fully re-rendered next frame.
    pub fn recycle_node(&mut self, id: SceneNodeId, prim: RenderPrimitive, x: i32, y: i32) {
        if let Some(node) = self.find_mut(id) {
            node.x = x;
            node.y = y;
            node.visible = true;
            node.opacity = 1.0;
            node.primitive = Some(prim);
            node.first_child = NIL;
            node.last_child = NIL;
            node.subtree_hash = 0;
        }
        self.mark_dirty(id, InvalidationClass::MeasureAndPlace);
    }

    /// Update only the text content on a recycled node (avoid full reset).
    /// Marks `PaintOnly`.
    pub fn set_text_content(&mut self, id: SceneNodeId, content: &'static str, color: Rgba8) {
        if let Some(node) = self.find_mut(id) {
            node.primitive = Some(RenderPrimitive::Text {
                content,
                font_scale: 2,
                color,
            });
        }
        self.mark_dirty(id, InvalidationClass::PaintOnly);
    }

    /// Update only the rect dimensions/color on a recycled node.
    /// Marks `PaintOnly`.
    pub fn set_rect(
        &mut self,
        id: SceneNodeId,
        width: u32,
        height: u32,
        radius: u32,
        color: Rgba8,
    ) {
        if let Some(node) = self.find_mut(id) {
            node.primitive = Some(RenderPrimitive::Rect {
                width,
                height,
                radius,
                color,
            });
        }
        self.mark_dirty(id, InvalidationClass::PaintOnly);
    }

    /// Return all currently unused node slots (previously removed nodes).
    /// These slots can be reused by recycling callers without growing the arena.
    pub fn free_slots(&self) -> Vec<SceneNodeId> {
        let mut free = Vec::new();
        for (idx, slot) in self.nodes.iter().enumerate().skip(1) {
            if slot.is_none() {
                free.push(SceneNodeId(idx as u64));
            }
        }
        free
    }

    // ------------------------------------------------------------------
    // GPU command generation — the single rendering authority
    // ------------------------------------------------------------------

    /// Generate GPU rendering commands for the given dirty nodes into an
    /// open render-pass encoder. Walks nodes in dirty-set order.
    ///
    /// The caller is responsible for:
    /// - Opening/closing the render pass on the `CommandBuffer`.
    /// - Adding wallpaper and any ambient rendering before/after this call.
    /// - Calling `mark_all_clean()` after a successful frame submission.
    ///
    /// Returns the number of commands emitted (0 = nothing changed).
    pub fn generate_commands_into(
        &mut self,
        dirty_set: &[SceneNodeId],
        extent_w: u32,
        extent_h: u32,
        encoder: &mut RenderCommandEncoder<'_>,
    ) -> Result<usize, GfxError> {
        // Take the text-tile scratch out of `self` so node borrows and the
        // scratch coexist; always restored (capacity is reused every frame).
        let mut tiles_scratch = core::mem::take(&mut self.text_tiles);
        let mut count: usize = 0;
        let mut result: Result<(), GfxError> = Ok(());
        for &id in dirty_set {
            let idx = id.0 as usize;
            let node = match self.nodes.get(idx).and_then(|n| n.as_ref()) {
                Some(n) => n,
                None => continue,
            };
            if !node.visible {
                continue;
            }
            if let Some(ref prim) = node.primitive {
                match Self::emit_primitive(
                    node,
                    prim,
                    extent_w,
                    extent_h,
                    encoder,
                    &mut tiles_scratch,
                ) {
                    Ok(n) => count += n,
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
        }
        self.text_tiles = tiles_scratch;
        result.map(|_| count)
    }

    /// World-space bounds of a node's primitive (clamped to ≥0, mirroring
    /// `emit_primitive`). `None` for nodes that occupy no compositable region
    /// (no primitive, the HW `Cursor`, or a `BackdropFilter`/`Group` without a
    /// clip). The unit of per-node damage and damage-intersection tests.
    pub fn node_world_bounds(node: &SceneNode) -> Option<TileRect> {
        let x = node.x.max(0) as u32;
        let y = node.y.max(0) as u32;
        match node.primitive.as_ref()? {
            RenderPrimitive::Rect { width, height, .. }
            | RenderPrimitive::StrokeRect { width, height, .. }
            | RenderPrimitive::Surface { width, height, .. } => {
                Some(TileRect { x, y, width: *width, height: *height })
            }
            RenderPrimitive::Text { content, font_scale, .. } => {
                let scale = (*font_scale).max(1);
                let advance = (5 + 1) * scale; // GLYPH_W(5)+1, see emit_primitive
                let w = (content.chars().count() as u32).saturating_mul(advance);
                Some(TileRect { x, y, width: w, height: 7 * scale })
            }
            RenderPrimitive::BackdropFilter { .. } | RenderPrimitive::Group { .. } => {
                node.clip.map(|c| TileRect {
                    x: c.x.as_i32().max(0) as u32,
                    y: c.y.as_i32().max(0) as u32,
                    width: c.width.as_i32().max(0) as u32,
                    height: c.height.as_i32().max(0) as u32,
                })
            }
            RenderPrimitive::Cursor { .. } => None,
        }
    }

    /// Damage-aware composite — the retained-compositor present path (#23).
    ///
    /// Re-emit every visible node whose world bounds intersect any `damage` rect,
    /// in back-to-front `render_order` (so z-order is reconstructed correctly
    /// within the damaged region — the caller passes `render_order_into`'s output).
    /// `Surface` blits are clipped to the damaged sub-rect, so a full-screen
    /// wallpaper/background node costs only the damaged area; bounded UI
    /// primitives (rects/text/glass) emit whole. Allocation-free: `render_order`
    /// is caller-owned scratch and the text-tile scratch is reused.
    ///
    /// Unlike [`generate_commands_into`] (which emits only the *dirty* nodes and
    /// is used for full repaints), this emits *all* nodes under the damage so an
    /// opaque change correctly repaints the layers beneath/above it.
    pub fn generate_commands_for_damage(
        &mut self,
        damage: &[TileRect],
        render_order: &[SceneNodeId],
        extent_w: u32,
        extent_h: u32,
        encoder: &mut RenderCommandEncoder<'_>,
    ) -> Result<usize, GfxError> {
        let mut tiles_scratch = core::mem::take(&mut self.text_tiles);
        let mut count: usize = 0;
        let mut result: Result<(), GfxError> = Ok(());
        for &id in render_order {
            let idx = id.0 as usize;
            let node = match self.nodes.get(idx).and_then(|n| n.as_ref()) {
                Some(n) => n,
                None => continue,
            };
            if !node.visible {
                continue;
            }
            let Some(prim) = node.primitive.as_ref() else {
                continue;
            };
            let Some(bounds) = Self::node_world_bounds(node) else {
                continue;
            };
            // Bounding intersection of the node with all damage rects it touches.
            let mut hit: Option<TileRect> = None;
            for d in damage {
                if let Some(i) = intersect_tile(bounds, *d) {
                    hit = Some(match hit {
                        Some(h) => union_tile(h, i),
                        None => i,
                    });
                }
            }
            let Some(hit) = hit else {
                continue;
            };
            let emit = match prim {
                // Clip the blit to the damaged sub-rect (offset the source).
                RenderPrimitive::Surface { src_x, src_y, .. } => {
                    let dx = hit.x.saturating_sub(bounds.x);
                    let dy = hit.y.saturating_sub(bounds.y);
                    match clamp_tile_to_extent(hit, extent_w, extent_h) {
                        Some(r) => encoder
                            .try_blit_surface(*src_x + dx, *src_y + dy, r.x, r.y, r.width, r.height)
                            .map(|_| 1),
                        None => Ok(0),
                    }
                }
                _ => Self::emit_primitive(node, prim, extent_w, extent_h, encoder, &mut tiles_scratch),
            };
            match emit {
                Ok(n) => count += n,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        self.text_tiles = tiles_scratch;
        result.map(|_| count)
    }

    /// Emit CB commands for a single `RenderPrimitive` at the node's position.
    ///
    /// All tile rects are clamped to `(extent_w, extent_h)`: the GPU render
    /// extent rejects any rect that overruns the framebuffer (and a single
    /// rejection would abort the entire frame), but an edge-touching panel
    /// shadow/blur legitimately extends to the screen border. Clamping keeps
    /// the visible portion and drops only the off-screen overrun.
    fn emit_primitive(
        node: &SceneNode,
        prim: &RenderPrimitive,
        extent_w: u32,
        extent_h: u32,
        encoder: &mut RenderCommandEncoder<'_>,
        tiles: &mut Vec<TileRect>,
    ) -> Result<usize, GfxError> {
        let x = node.x.max(0) as u32;
        let y = node.y.max(0) as u32;
        let clamp = |r: TileRect| clamp_tile_to_extent(r, extent_w, extent_h);

        match prim {
            RenderPrimitive::Rect {
                width,
                height,
                radius,
                color,
            } => {
                match clamp(TileRect {
                    x,
                    y,
                    width: *width,
                    height: *height,
                }) {
                    Some(r) => {
                        encoder.try_fill_sdf_rounded_rect(
                            r,
                            *radius,
                            RgbaColor::new(color.r, color.g, color.b, color.a),
                        )?;
                        Ok(1)
                    }
                    None => Ok(0),
                }
            }
            RenderPrimitive::StrokeRect {
                width,
                height,
                radius,
                stroke_width,
                color,
            } => {
                // Stroke rendered as filled rect in the border color;
                // an interior fill covers the center when the caller
                // emits a second Rect with a smaller radius.
                let _ = stroke_width;
                match clamp(TileRect {
                    x,
                    y,
                    width: *width,
                    height: *height,
                }) {
                    Some(r) => {
                        encoder.try_fill_sdf_rounded_rect(
                            r,
                            *radius,
                            RgbaColor::new(color.r, color.g, color.b, color.a),
                        )?;
                        Ok(1)
                    }
                    None => Ok(0),
                }
            }
            RenderPrimitive::Surface {
                surface_handle: _,
                src_x,
                src_y,
                width,
                height,
            } => {
                encoder.try_blit_surface(*src_x, *src_y, x, y, *width, *height)?;
                Ok(1)
            }
            RenderPrimitive::Text {
                content,
                font_scale,
                color,
            } => {
                // Rasterize the 5x7 bitmap font into solid tiles and emit one
                // DrawTiles command per run (a single colored fill batch — well
                // within the per-frame command budget). Text shaping is upstream;
                // this is the GPU draw of pre-shaped content.
                const GLYPH_W: u32 = 5;
                const GLYPH_H: u32 = 7;
                const MAX_GLYPH_TILES: usize = 1024; // CommandBuffer MAX_TILE_RECTS
                let scale = (*font_scale).max(1);
                let advance = (GLYPH_W + 1) * scale;
                tiles.clear(); // reused scratch — capacity persists across frames
                let mut pen_x = x;
                'chars: for ch in content.chars() {
                    let rows = crate::bitmap_font::bitmap_font_5x7(ch);
                    for (ry, bits) in rows.iter().enumerate() {
                        if (ry as u32) >= GLYPH_H {
                            break;
                        }
                        // Coalesce consecutive on-pixels in this row into a single
                        // wide tile — far fewer commands/bytes than one tile per
                        // pixel (which can overflow the serialized CB buffer).
                        let mut cx = 0u32;
                        while cx < GLYPH_W {
                            // Leftmost column is the MSB of the 5-bit row.
                            if (bits >> (GLYPH_W - 1 - cx)) & 1 == 1 {
                                let run_start = cx;
                                while cx < GLYPH_W && (bits >> (GLYPH_W - 1 - cx)) & 1 == 1 {
                                    cx += 1;
                                }
                                let tile = TileRect {
                                    x: pen_x + run_start * scale,
                                    y: y + ry as u32 * scale,
                                    width: (cx - run_start) * scale,
                                    height: scale,
                                };
                                if let Some(r) = clamp(tile) {
                                    tiles.push(r);
                                    if tiles.len() >= MAX_GLYPH_TILES {
                                        break 'chars;
                                    }
                                }
                            } else {
                                cx += 1;
                            }
                        }
                    }
                    pen_x = pen_x.saturating_add(advance);
                }
                if tiles.is_empty() {
                    return Ok(0);
                }
                encoder
                    .try_draw_tiles(&tiles, RgbaColor::new(color.r, color.g, color.b, color.a))?;
                Ok(1)
            }
            RenderPrimitive::BackdropFilter {
                blur_radius,
                saturation_percent,
            } => {
                // BackdropFilter applies to the node's clipped region.
                if let Some(clip) = node.clip {
                    let cx = clip.x.as_i32().max(0) as u32;
                    let cy = clip.y.as_i32().max(0) as u32;
                    let cw = clip.width.as_i32().max(0) as u32;
                    let ch = clip.height.as_i32().max(0) as u32;
                    if let Some(r) = clamp(TileRect {
                        x: cx,
                        y: cy,
                        width: cw,
                        height: ch,
                    }) {
                        encoder.try_blur_backdrop(r, *blur_radius, *saturation_percent)?;
                        return Ok(1);
                    }
                }
                Ok(0)
            }
            RenderPrimitive::Group { shadow } => {
                // Groups are containers — children emit their own commands.
                // If a shadow is present, emit a blurred offset fill as the shadow.
                if let Some(s) = shadow {
                    let sx = (x as i32 + s.offset_x.as_i32()).max(0) as u32;
                    let sy = (y as i32 + s.offset_y.as_i32()).max(0) as u32;
                    // Shadow rendered as a filled rect with the shadow color,
                    // offset and expanded by the blur radius to approximate spread.
                    let br = s.blur_radius.as_i32().max(0) as u32;
                    let mut emitted = 0;
                    if let Some(clip) = node.clip {
                        let cw = clip.width.as_i32().max(0) as u32;
                        let ch = clip.height.as_i32().max(0) as u32;
                        if cw > 0 && ch > 0 && br > 0 {
                            // Blur the shadow region behind the group.
                            if let Some(r) = clamp(TileRect {
                                x: sx.saturating_sub(br),
                                y: sy.saturating_sub(br),
                                width: (cw + br * 2).min(8192),
                                height: (ch + br * 2).min(8192),
                            }) {
                                encoder.try_blur_backdrop(r, br, 0)?;
                                emitted += 1;
                            }
                        }
                        // Fill shadow rect at offset.
                        if let Some(r) = clamp(TileRect {
                            x: sx,
                            y: sy,
                            width: cw,
                            height: ch,
                        }) {
                            encoder.try_fill_sdf_rounded_rect(
                                r,
                                4,
                                RgbaColor::new(s.color.r, s.color.g, s.color.b, s.color.a),
                            )?;
                            emitted += 1;
                        }
                    }
                    Ok(emitted)
                } else {
                    Ok(0)
                }
            }
            RenderPrimitive::Cursor {
                hotspot_x: _,
                hotspot_y: _,
            } => {
                // Cursor blending is handled outside the scene graph
                // via `shell.update_cursor()` + explicit `BlendCursor`.
                Ok(0)
            }
        }
    }
}

impl Default for SceneGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Allocation-free iterator over a node's children (insertion / z order),
/// following the intrusive sibling links.
pub struct ChildIter<'a> {
    graph: &'a SceneGraph,
    next: SceneNodeId,
}

impl<'a> Iterator for ChildIter<'a> {
    type Item = SceneNodeId;

    fn next(&mut self) -> Option<SceneNodeId> {
        if self.next == NIL {
            return None;
        }
        let id = self.next;
        self.next = self.graph.find(id).map(|n| n.next_sibling).unwrap_or(NIL);
        Some(id)
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
        let kids: Vec<SceneNodeId> = g.children(root).collect();
        assert_eq!(kids, vec![child]);
        let child_node = g.find(child).unwrap();
        assert_eq!(child_node.parent, Some(root));
    }

    #[test]
    fn sibling_links_preserve_order_across_removal() {
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let a = make_node(&mut g, Some(root));
        let b = make_node(&mut g, Some(root));
        let c = make_node(&mut g, Some(root));
        let kids: Vec<SceneNodeId> = g.children(root).collect();
        assert_eq!(kids, vec![a, b, c]);

        // Removing the middle child keeps order; appending re-links at the end.
        g.remove(b);
        let kids: Vec<SceneNodeId> = g.children(root).collect();
        assert_eq!(kids, vec![a, c]);
        let d = make_node(&mut g, Some(root));
        let kids: Vec<SceneNodeId> = g.children(root).collect();
        assert_eq!(kids, vec![a, c, d]);
        assert_eq!(g.child_count(root), 3);
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
        assert_eq!(
            g.find(id).unwrap().invalidation,
            InvalidationClass::MeasureAndPlace
        );

        // Mark clean, then move
        g.mark_all_clean();
        g.set_position(id, 10, 20);
        assert_eq!(
            g.find(id).unwrap().invalidation,
            InvalidationClass::PlaceOnly
        );
    }

    #[test]
    fn set_opacity_marks_paint_only() {
        let mut g = make_graph();
        let id = make_node(&mut g, None);
        g.mark_all_clean();
        g.set_opacity(id, 0.5);
        assert_eq!(
            g.find(id).unwrap().invalidation,
            InvalidationClass::PaintOnly
        );
    }

    #[test]
    fn dirty_child_propagates_to_parent() {
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let child = make_node(&mut g, Some(root));

        g.mark_all_clean();
        assert_eq!(g.find(root).unwrap().invalidation, InvalidationClass::Clean);
        assert_eq!(
            g.find(child).unwrap().invalidation,
            InvalidationClass::Clean
        );

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

        assert_ne!(
            hash_before, hash_after,
            "hash should change when primitive changes"
        );
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

        g.apply_animation_update_to(
            id,
            SceneUpdate {
                layer_id: LayerId(id.0),
                property: AnimProp::TranslateX,
                progress: 0.0,
                value: 42.0,
            },
        );

        let node = g.find(id).unwrap();
        assert_eq!(node.x, 42);
        assert_eq!(node.invalidation, InvalidationClass::PlaceOnly);
    }

    #[test]
    fn animation_update_sets_opacity() {
        let mut g = make_graph();
        let id = make_node(&mut g, None);
        g.mark_all_clean();

        g.apply_animation_update_to(
            id,
            SceneUpdate {
                layer_id: LayerId(id.0),
                property: AnimProp::Opacity,
                progress: 0.0,
                value: 0.3,
            },
        );

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
        assert!(
            dirty.is_empty(),
            "no nodes should be dirty after mark_all_clean"
        );
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
            RenderPrimitive::Rect {
                width: 0,
                height: 0,
                radius: 0,
                color: Rgba8::new(0, 0, 0, 0),
            }
            .tag(),
            RenderPrimitive::StrokeRect {
                width: 0,
                height: 0,
                radius: 0,
                stroke_width: 0,
                color: Rgba8::new(0, 0, 0, 0),
            }
            .tag(),
            RenderPrimitive::Surface {
                surface_handle: 0,
                src_x: 0,
                src_y: 0,
                width: 0,
                height: 0,
            }
            .tag(),
            RenderPrimitive::Text {
                content: "",
                font_scale: 0,
                color: Rgba8::new(0, 0, 0, 0),
            }
            .tag(),
            RenderPrimitive::BackdropFilter {
                blur_radius: 0,
                saturation_percent: 0,
            }
            .tag(),
            RenderPrimitive::Group { shadow: None }.tag(),
            RenderPrimitive::Cursor {
                hotspot_x: 0,
                hotspot_y: 0,
            }
            .tag(),
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

    #[test]
    fn dirty_set_is_o_dirty_not_o_nodes() {
        // 1 root + 500 leaves; touching ONE leaf must rehash only that leaf
        // and its ancestor chain — never the whole graph.
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let mut leaves = alloc::vec::Vec::new();
        for _ in 0..500 {
            leaves.push(make_node(&mut g, Some(root)));
        }
        let mut out = alloc::vec::Vec::new();
        g.compute_dirty_set_into(&mut out);
        g.mark_all_clean();

        g.set_position(leaves[250], 42, 42);
        g.compute_dirty_set_into(&mut out);
        // Dirty set: the leaf + its ancestor (root).
        assert_eq!(out.len(), 2);
        assert!(out.contains(&leaves[250]));
        assert!(out.contains(&root));
        // O(dirty) invariant: only the listed nodes were hashed (×passes),
        // far below the 501 nodes in the graph.
        assert!(
            g.last_hash_recomputes() <= 8,
            "hash recomputes {} not O(dirty)",
            g.last_hash_recomputes()
        );
    }

    #[test]
    fn dirty_propagates_class_to_ancestors() {
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let mid = make_node(&mut g, Some(root));
        let leaf = make_node(&mut g, Some(mid));
        g.mark_all_clean();

        g.set_opacity(leaf, 0.5); // PaintOnly
        let dirty = g.compute_dirty_set();
        assert!(dirty.contains(&leaf) && dirty.contains(&mid) && dirty.contains(&root));
        assert_eq!(
            g.find(root).unwrap().invalidation,
            InvalidationClass::PaintOnly
        );
        assert_eq!(
            g.find(mid).unwrap().invalidation,
            InvalidationClass::PaintOnly
        );
    }

    #[test]
    fn steady_state_frames_do_not_allocate() {
        // Bump-allocator discipline: after warm-up, the per-frame buffers
        // (caller dirty list + internal dirty list) must not grow — Vec::clear
        // keeps capacity, so capacities must be stable across many frames.
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let mut kids = alloc::vec::Vec::new();
        for _ in 0..32 {
            kids.push(make_node(&mut g, Some(root)));
        }
        let mut out = alloc::vec::Vec::with_capacity(64);
        // Warm-up frame.
        g.compute_dirty_set_into(&mut out);
        g.mark_all_clean();
        let warm_cap = out.capacity();

        for frame in 0..200 {
            g.set_position(kids[frame % 32], frame as i32, 0);
            g.set_opacity(kids[(frame + 7) % 32], 0.9);
            g.compute_dirty_set_into(&mut out);
            assert!(!out.is_empty());
            g.mark_all_clean();
        }
        assert_eq!(out.capacity(), warm_cap, "dirty-out buffer reallocated");
    }

    #[test]
    fn mark_all_clean_then_empty_dirty_set() {
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let child = make_node(&mut g, Some(root));
        g.set_opacity(child, 0.4);
        g.mark_all_clean();
        let mut out = alloc::vec::Vec::new();
        g.compute_dirty_set_into(&mut out);
        assert!(out.is_empty());
        assert_eq!(g.last_hash_recomputes(), 0);
    }

    #[test]
    fn removed_node_does_not_appear_in_dirty_set() {
        let mut g = make_graph();
        let root = make_node(&mut g, None);
        let child = make_node(&mut g, Some(root));
        g.mark_all_clean();
        g.set_opacity(child, 0.4); // dirty, then remove before the frame
        g.remove(child);
        let dirty = g.compute_dirty_set();
        assert!(!dirty.contains(&child));
        // The structural change still re-renders the parent.
        assert!(dirty.contains(&root));
    }

    #[test]
    fn render_order_into_reuses_buffer_and_orders_by_insertion() {
        let mut g = make_graph();
        let a = make_node(&mut g, None);
        let b = make_node(&mut g, Some(a));
        let c = make_node(&mut g, Some(a));
        let mut out = alloc::vec::Vec::with_capacity(8);
        g.render_order_into(&mut out);
        assert_eq!(out, alloc::vec![a, b, c]);
        let cap = out.capacity();
        g.render_order_into(&mut out);
        assert_eq!(out.capacity(), cap);
    }

    #[test]
    fn node_world_bounds_per_primitive() {
        let mut g = make_graph();
        // Rect: bounds = position + width/height.
        let r = make_node(&mut g, None);
        g.set_position(r, 100, 50);
        g.set_primitive(r, rect_prim(40, 20));
        let rb = SceneGraph::node_world_bounds(g.find(r).unwrap()).unwrap();
        assert_eq!((rb.x, rb.y, rb.width, rb.height), (100, 50, 40, 20));

        // Cursor occupies no compositable region (HW plane).
        let c = make_node(&mut g, None);
        g.set_primitive(c, RenderPrimitive::Cursor { hotspot_x: 0, hotspot_y: 0 });
        assert!(SceneGraph::node_world_bounds(g.find(c).unwrap()).is_none());

        // No primitive → no bounds.
        let e = make_node(&mut g, None);
        assert!(SceneGraph::node_world_bounds(g.find(e).unwrap()).is_none());
    }

    #[test]
    fn tile_intersection_and_union() {
        let a = TileRect { x: 0, y: 0, width: 100, height: 100 };
        let b = TileRect { x: 50, y: 50, width: 100, height: 100 };
        // Overlap.
        assert_eq!(
            intersect_tile(a, b),
            Some(TileRect { x: 50, y: 50, width: 50, height: 50 })
        );
        // Disjoint → None.
        let far = TileRect { x: 200, y: 200, width: 10, height: 10 };
        assert_eq!(intersect_tile(a, far), None);
        // Edge-touching (no overlap).
        let edge = TileRect { x: 100, y: 0, width: 10, height: 10 };
        assert_eq!(intersect_tile(a, edge), None);
        // Union bounds both.
        let u = union_tile(a, b);
        assert_eq!((u.x, u.y, u.width, u.height), (0, 0, 150, 150));
    }

    #[test]
    fn damage_intersection_selects_only_touched_nodes() {
        // A tiny damage rect over one node must intersect only that node's
        // bounds — the per-node-damage invariant that keeps a far-away change
        // off the rest of the scene.
        let mut g = make_graph();
        let near = make_node(&mut g, None);
        g.set_position(near, 10, 10);
        g.set_primitive(near, rect_prim(20, 20));
        let far = make_node(&mut g, None);
        g.set_position(far, 500, 500);
        g.set_primitive(far, rect_prim(20, 20));

        let damage = TileRect { x: 12, y: 12, width: 4, height: 4 };
        let nb = SceneGraph::node_world_bounds(g.find(near).unwrap()).unwrap();
        let fb = SceneGraph::node_world_bounds(g.find(far).unwrap()).unwrap();
        assert!(intersect_tile(nb, damage).is_some());
        assert!(intersect_tile(fb, damage).is_none());
    }
}
