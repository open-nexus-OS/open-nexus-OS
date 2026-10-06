// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: modal semantics on the app-owned `.overlay()` primitive (TASK-0074, ADR-0068):
//! the OVERLAY STACK a scene emit produces — every `.overlay(modal|transient)` container in
//! emit order (later = on top) — with the bounded modal depth, the topmost modal that confines
//! hit-testing and text focus, and the transient layers' declared timeouts. The stack is a
//! DERIVED view of the scene, never a second state: an overlay is on it exactly while the
//! state that emits it holds, and it leaves only through its own `on Dismiss` handler
//! (`View::dismiss_top`). Pure data — host-tested.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: inline + `tests/dsl_conformance/tests/overlays.rs`

use crate::RtError;
use alloc::vec::Vec;

/// The deepest modal nesting a scene may emit (TASK-0074 invariant): the fifth modal is a
/// runtime error, not a sixth layer — a dialog over a dialog over a dialog over a dialog is a
/// design bug, and the bound keeps the confinement walk O(1).
pub const MODAL_DEPTH_MAX: usize = 4;

/// Ceiling of `.dismissAfter(ms)` — a transient that lives longer is a modal in disguise.
pub const DISMISS_AFTER_MAX_MS: u32 = 60_000;

/// What `.overlay(kind)` declared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverlayKind {
    /// Bare `.overlay()`: the full-bleed out-of-flow layer, no modal semantics.
    #[default]
    Plain,
    /// `.overlay(modal)`: input and text focus are confined to the layer's subtree while it
    /// is the topmost modal; ESC and a backdrop tap fire its `on Dismiss`.
    Modal,
    /// `.overlay(transient)`: a toast-class layer; the host's timer fires its `on Dismiss`
    /// after `.dismissAfter(ms)`. It confines nothing.
    Transient,
}

/// Why an overlay's `on Dismiss` fired — reported to the host for its marker, never a payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DismissReason {
    Escape,
    Backdrop,
    Timeout,
}

impl DismissReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Escape => "escape",
            Self::Backdrop => "backdrop",
            Self::Timeout => "timeout",
        }
    }
}

/// One kinded overlay of the current scene.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayEntry {
    /// Child-index path of the layer's node (the same path its handlers carry).
    pub path: Vec<u32>,
    /// The store instance the layer was emitted in (its handlers dispatch there).
    pub instance: u64,
    pub kind: OverlayKind,
    /// `.dismissAfter(ms)` of a transient layer (bounded at emit).
    pub dismiss_after_ms: Option<u32>,
    /// Pre-order box id of the layer's node in the emitted scene (0 = unresolved).
    pub box_id: usize,
}

/// The scene's kinded overlays in emit order (the last modal is the topmost).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverlayStack {
    entries: Vec<OverlayEntry>,
}

impl OverlayStack {
    #[must_use]
    pub const fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Records a kinded overlay; the modal beyond `MODAL_DEPTH_MAX` is refused.
    ///
    /// # Errors
    /// `RtError::OverlayDepth` when a fifth modal is pushed.
    pub fn push(&mut self, entry: OverlayEntry) -> Result<(), RtError> {
        if entry.kind == OverlayKind::Modal && self.modal_depth() >= MODAL_DEPTH_MAX {
            return Err(RtError::OverlayDepth);
        }
        self.entries.push(entry);
        Ok(())
    }

    #[must_use]
    pub fn entries(&self) -> &[OverlayEntry] {
        &self.entries
    }

    pub(crate) fn entries_mut(&mut self) -> &mut [OverlayEntry] {
        &mut self.entries
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many modal layers the scene holds.
    #[must_use]
    pub fn modal_depth(&self) -> usize {
        self.entries.iter().filter(|e| e.kind == OverlayKind::Modal).count()
    }

    /// The topmost modal (the last one emitted) — input and focus confine to its subtree.
    #[must_use]
    pub fn top_modal(&self) -> Option<&OverlayEntry> {
        self.entries.iter().rev().find(|e| e.kind == OverlayKind::Modal)
    }

    /// The transient layers, in emit order.
    pub fn transients(&self) -> impl Iterator<Item = &OverlayEntry> {
        self.entries.iter().filter(|e| e.kind == OverlayKind::Transient)
    }

    /// The entry at `path`, if the scene holds one there.
    #[must_use]
    pub fn at_path(&self, path: &[u32]) -> Option<&OverlayEntry> {
        self.entries.iter().find(|e| e.path == path)
    }
}

/// Records the node being emitted when its modifiers declare a kind (emit order = stacking
/// order; the bound refuses the fifth modal). A disabled layer is still a layer — confinement
/// is about where input may land, not whether the layer takes it.
pub(crate) fn note_kinded(
    ctx: &mut crate::emit::EmitCtx<'_, '_>,
    mods: &crate::registry::Mods,
) -> Result<(), RtError> {
    let Some(kind) = mods.overlay.filter(|k| *k != OverlayKind::Plain) else {
        return Ok(());
    };
    ctx.overlays.push(OverlayEntry {
        path: ctx.path.clone(),
        instance: ctx.instance,
        kind,
        dismiss_after_ms: mods.dismiss_after,
        box_id: 0,
    })
}

/// Whether a handler at `path` is reachable while `confine` (the topmost modal's path) is in
/// force: only the modal's own subtree takes input. `None` = nothing is confined.
#[must_use]
pub fn reachable(path: &[u32], confine: Option<&[u32]>) -> bool {
    confine.is_none_or(|prefix| path.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn entry(path: &[u32], kind: OverlayKind) -> OverlayEntry {
        OverlayEntry { path: path.to_vec(), instance: 0, kind, dismiss_after_ms: None, box_id: 0 }
    }

    #[test]
    fn the_last_modal_is_on_top_and_transients_do_not_count() {
        let mut stack = OverlayStack::new();
        stack.push(entry(&[0, 1], OverlayKind::Modal)).unwrap();
        stack.push(entry(&[0, 2], OverlayKind::Transient)).unwrap();
        stack.push(entry(&[0, 1, 3], OverlayKind::Modal)).unwrap();
        assert_eq!(stack.modal_depth(), 2);
        assert_eq!(stack.top_modal().map(|e| e.path.as_slice()), Some(&[0, 1, 3][..]));
        assert_eq!(stack.transients().count(), 1);
        assert!(stack.at_path(&[0, 2]).is_some());
    }

    #[test]
    fn test_reject_the_fifth_modal() {
        let mut stack = OverlayStack::new();
        for i in 0..MODAL_DEPTH_MAX as u32 {
            stack.push(entry(&[i], OverlayKind::Modal)).unwrap();
        }
        assert_eq!(stack.push(entry(&[9], OverlayKind::Modal)), Err(RtError::OverlayDepth));
        // A transient still fits: only modals are bounded.
        assert_eq!(stack.push(entry(&[10], OverlayKind::Transient)), Ok(()));
    }

    #[test]
    fn confinement_is_a_path_prefix() {
        assert!(reachable(&[0, 1, 2], Some(&[0, 1])));
        assert!(!reachable(&[0, 2], Some(&[0, 1])));
        assert!(!reachable(&[0], Some(&[0, 1])), "an ancestor is outside the modal");
        assert!(reachable(&[7], None));
        let _ = vec![0u32];
    }
}
