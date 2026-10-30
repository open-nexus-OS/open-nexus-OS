// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: undo and redo for one focused field (TASK-0346 P2): the value and the caret before
//! each change, bounded to [`HISTORY_MAX`] steps. A run of typing — or of deleting — is ONE step:
//! a caret move, a change of the other kind, a paste or a cut ends it, as text fields do on every
//! desktop. A new change after an undo forgets the redo. The field's value lives in its app's
//! store; the runtime records before it writes and restores what an undo hands back.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

use crate::Edit;

/// Undo steps one field keeps (the oldest is forgotten first).
pub const HISTORY_MAX: usize = 64;

/// What a change was: typing and deleting each run into one step; anything else is a step alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Typing,
    Deleting,
    /// A paste, a cut, a replacement: one step each.
    Other,
}

/// A state to come back to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub value: String,
    pub edit: Edit,
}

/// One field's history.
#[derive(Clone, Debug, Default)]
pub struct History {
    undo: VecDeque<Snapshot>,
    redo: Vec<Snapshot>,
    run: Option<Change>,
}

impl History {
    #[must_use]
    pub const fn new() -> Self {
        Self { undo: VecDeque::new(), redo: Vec::new(), run: None }
    }

    /// Before a change of `kind` to (`value`, `edit`): the state to come back to — unless the
    /// change continues a run of the same kind.
    pub fn record(&mut self, value: &str, edit: Edit, kind: Change) {
        if kind != Change::Other && self.run == Some(kind) {
            return;
        }
        self.push_undo(Snapshot { value: String::from(value), edit });
        self.redo.clear();
        self.run = (kind != Change::Other).then_some(kind);
    }

    /// The caret moved without a change: a run ends.
    pub fn moved(&mut self) {
        self.run = None;
    }

    /// Undo from (`value`, `edit`): the state to restore — the current one is kept for a redo.
    pub fn undo(&mut self, value: &str, edit: Edit) -> Option<Snapshot> {
        let back = self.undo.pop_back()?;
        self.redo.push(Snapshot { value: String::from(value), edit });
        self.run = None;
        Some(back)
    }

    /// Redo from (`value`, `edit`): the state an undo left — the current one goes back on undo.
    pub fn redo(&mut self, value: &str, edit: Edit) -> Option<Snapshot> {
        let forward = self.redo.pop()?;
        self.push_undo(Snapshot { value: String::from(value), edit });
        self.run = None;
        Some(forward)
    }

    fn push_undo(&mut self, snapshot: Snapshot) {
        if self.undo.len() == HISTORY_MAX {
            self.undo.pop_front();
        }
        self.undo.push_back(snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(caret: usize) -> Edit {
        Edit { caret, anchor: None }
    }

    #[test]
    fn a_typed_word_is_one_undo_step_and_comes_back_with_redo() {
        let mut h = History::new();
        for (i, before) in ["", "h", "ha", "hal"].iter().enumerate() {
            h.record(before, at(i), Change::Typing);
        }
        let back = h.undo("hall", at(4)).expect("one step");
        assert_eq!((back.value.as_str(), back.edit), ("", at(0)));
        assert_eq!(h.undo("", at(0)), None, "the run was one step");
        let forward = h.redo("", at(0)).expect("redo");
        assert_eq!(forward.value, "hall");
    }

    #[test]
    fn a_move_or_another_kind_ends_the_run() {
        let mut h = History::new();
        h.record("", at(0), Change::Typing);
        h.record("a", at(1), Change::Typing);
        h.moved();
        h.record("ab", at(0), Change::Typing);
        h.record("xab", at(1), Change::Deleting);
        h.record("ab", at(0), Change::Other);
        h.record("ab", at(0), Change::Other);
        let steps = core::iter::from_fn(|| h.undo("", at(0))).count();
        assert_eq!(steps, 5, "typing | typing after the move | deleting | other | other");
    }

    #[test]
    fn test_reject_a_redo_after_a_new_change_and_history_past_its_bound() {
        let mut h = History::new();
        h.record("a", at(1), Change::Other);
        let _ = h.undo("ab", at(2));
        h.record("a", at(1), Change::Typing);
        assert_eq!(h.redo("ax", at(2)), None, "a new change forgets the redo");
        let mut long = History::new();
        for i in 0..(HISTORY_MAX + 10) {
            long.record("v", at(i), Change::Other);
        }
        let steps = core::iter::from_fn(|| long.undo("v", at(0))).count();
        assert_eq!(steps, HISTORY_MAX, "bounded: the oldest are forgotten");
    }
}
