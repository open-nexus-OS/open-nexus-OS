// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Animation intents — the runtime's carrier for the declared, decided
//! `.animate`/`.transition`/`.effect` DSL modifiers (docs/dev/dsl/modifiers.md
//! "Motion", docs/dev/ui/foundations/animation.md).
//!
//! An intent is a PURE, value-typed description stamped onto an animated node
//! at emit time: which motion token, which category, and — for
//! `.animate`/`.effect` — the current committed snapshot of the driving
//! value/trigger expression. It carries NO clock and NO physics: the DSL stays
//! pure (principles.md §4) and the HOST (app-host `AnimationDriver`) owns time,
//! interpolation, and paint. The intent is resolved to a scene `node_id` after
//! emit (`View::animations`, via the same pre-order `path_to_box_id` walk the
//! interaction handlers use).

/// Which motion modifier produced this intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimKind {
    /// `.animate(token, value: expr)` — animate state-driven property changes.
    Animate,
    /// `.transition(token)` — insert/remove/open/close lifecycle motion.
    Transition,
    /// `.effect(token, trigger: expr)` — bounded attention effect on change.
    Effect,
    /// A CONTINUOUS, self-sustaining loop — NOT from a modifier: emitted by the
    /// runtime for an inherently-animated KIT WIDGET (e.g. a `Skeleton` loading
    /// placeholder) whose resting frame should breathe until it leaves the tree.
    /// The host runs it as a paint-time transform loop (no per-frame re-emit —
    /// the app-host bump heap never frees), the same `AnimationDriver` + per-node
    /// transform path the modifiers use. Widgets whose motion is spoke rotation
    /// or a shimmer sweep (paths / clipped translate) are deferred to the
    /// compositor layer transform (Track C), not this CPU paint loop.
    Loop,
}

/// One animation stamped on a node. `Copy`/`Eq` so the host can diff intents
/// across re-emits with zero allocation (steady-state alloc-bounded contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimIntent {
    pub kind: AnimKind,
    /// `animation::MotionToken` id (append-only stable wire id).
    pub token: u8,
    /// Committed snapshot of the driving `value:` / `trigger:` expression at
    /// emit time (Bool → 0/1, Int → the value, everything else → 0). For
    /// `.transition` (no expr) this is 1 = "present in the tree this frame".
    /// The host animates when this changes vs the last emit of the same node.
    pub value: i32,
}

impl AnimIntent {
    #[must_use]
    pub fn new(kind: AnimKind, token: u8, value: i32) -> Self {
        Self { kind, token, value }
    }
}
