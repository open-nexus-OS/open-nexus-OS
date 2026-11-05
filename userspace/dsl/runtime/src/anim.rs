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
    /// runtime for an inherently-animated KIT WIDGET (`Skeleton` shimmer,
    /// indeterminate `ProgressBar` pip, `Spinner` spoke rotation) that runs
    /// until the node leaves the tree. The host runs it as a paint-time
    /// transform loop (no per-frame re-emit, which would allocate every
    /// frame), keyed by the intent VALUE's sub-kind (`LOOP_*`): the widgets'
    /// fixed builder structure makes the animated part a stable pre-order
    /// child offset, so the loop targets child boxes without any rebuild.
    Loop,
}

/// `AnimIntent::value` sub-kinds for [`AnimKind::Loop`] (the host maps each to
/// its paint-time loop implementation; the widget builders' structure is the
/// contract — update together):
/// Whole-node opacity breathe (the generic resting pulse).
pub const LOOP_BREATHE: i32 = 0;
/// TranslateX sawtooth on the sole child (`root+1`): the Skeleton shimmer
/// band / the indeterminate ProgressBar pip sweeping across the track.
pub const LOOP_SWEEP: i32 = 1;
/// Stepped opacity rotation across the 12 spoke children (`root+1..=root+12`):
/// the Spinner. Pure paint-time stepping on a time grid — no springs.
pub const LOOP_CAROUSEL: i32 = 2;
/// Spoke count the carousel rotates over (the Spinner builder's SPOKES len).
pub const LOOP_CAROUSEL_SPOKES: usize = 12;

/// One animation stamped on a node. `Copy`/`Eq` so the host can diff intents
/// across re-emits with zero allocation (steady-state alloc-bounded contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimIntent {
    pub kind: AnimKind,
    /// `animation::MotionToken` id (append-only stable wire id).
    pub token: u8,
    /// Committed snapshot of the driving `value:` / `trigger:` expression at
    /// emit time. For `.transition` (no expr) it is the node's IDENTITY —
    /// present (never 0) and which node: a stable hash of its place in the tree
    /// ([`transition_identity`]). The host animates when this changes vs the
    /// last emit of the same pre-order number: the same node re-emitted keeps
    /// it, another node landing on that number (a modal closing as a toast
    /// opens — overlays are numbered last) differs, and enters.
    ///
    /// What gets folded in here depends on `kind`, because the two modifiers
    /// ask different questions of the same expression (see `emit::stamp_anim`):
    ///
    ///   * [`AnimKind::Animate`] — PRESENCE. `Bool` → 0/1, `Int`/`Fx` → the
    ///     integer value; anything else has no presence reading and the intent
    ///     is **not stamped at all**, so an un-foldable `value:` leaves the
    ///     node un-animated instead of resting it at opacity 0 (invisible).
    ///   * [`AnimKind::Effect`] — CHANGE only. Numbers as above, `Str` as a
    ///     stable hash, so a string trigger actually fires.
    pub value: i32,
}

/// A `.transition`'s intent value: the node's identity — a stable FNV-1a hash of its child-index
/// path in the emitted scene, never 0 (0 reads as absent). Two nodes at one pre-order number in two
/// emits (the numbering is positional: an overlay is numbered after every in-flow node) hash apart
/// when their paths differ; the same node keeps its value across re-emits.
#[must_use]
pub fn transition_identity(path: &[u32]) -> i32 {
    let mut h: u32 = 0x811c_9dc5;
    for &i in path {
        for b in i.to_le_bytes() {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    (h | 1) as i32
}

impl AnimIntent {
    #[must_use]
    pub fn new(kind: AnimKind, token: u8, value: i32) -> Self {
        Self { kind, token, value }
    }
}

#[cfg(test)]
mod tests {
    use super::transition_identity;

    /// A transition names its node by its place: two places, two identities; the same place, the
    /// same identity across emits; never 0 (absent).
    #[test]
    fn a_transition_names_its_node_by_its_place() {
        let alert = transition_identity(&[0, 3, 0]);
        let toast = transition_identity(&[0, 3, 4]);
        assert_ne!(alert, toast, "two nodes, two identities");
        assert_eq!(alert, transition_identity(&[0, 3, 0]), "the same node keeps its identity");
        assert!([alert, toast, transition_identity(&[])].iter().all(|v| *v != 0), "never absent");
    }
}
