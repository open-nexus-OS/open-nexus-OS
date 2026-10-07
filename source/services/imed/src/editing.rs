// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: imed's text-field editing paths (TASK-0067B, RFC-0075 amendment): a whole-text
//! INSERT from the keyboard's clipboard cards (`OP_INSERT` on the OSK endpoint) and the
//! keyboard's EDITING COMMANDS (navigation, Shift-selection, Ctrl+A/C/X/V). Neither enters
//! composition. An editing command first commits whatever is being composed — the command
//! then acts on the text the user sees — and is passed to the focused field, which owns
//! caret, selection and the clipboard calls. Copy and cut never leave a password field.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `src/tests.rs` (insert + editing commands)

use ime_core::{ImeAction, ImeEngine, ImeKey, TextRun};
use nexus_wire::imed as wire;

use crate::{CommitText, ImedCore, KeyPushes, StepEcho};

impl ImedCore {
    /// Inserts `text` into the focused field as ONE commit (the OSK's clipboard cards,
    /// TASK-0067B). Delivery is focus-gated like every commit, and a `FIELD_KIND_NONE` focus
    /// (a window without a text field) takes nothing. Any half-typed composition is dropped
    /// first so the card's text lands alone, and the strip is cleared if it was showing.
    /// Nothing is learned: a pasted card is not a typed word.
    pub fn insert(&mut self, text: &str) -> (Option<KeyPushes>, StepEcho) {
        if text.is_empty() {
            return (None, StepEcho { commit: CommitText::default() });
        }
        // Like a typed key, the step's commit is echoed whether or not a field
        // takes it (the osk probe reads the echo); DELIVERY is focus-gated.
        let echo = StepEcho { commit: CommitText::from_str(text) };
        let Some(focus) = self.focus.filter(|f| f.field_kind != wire::FIELD_KIND_NONE) else {
            return (None, echo);
        };
        self.engine.reset();
        let (preedit, candidates) = if self.strip_dirty {
            self.strip_dirty = false;
            (Some(TextRun::empty()), Some(ime_core::CandidatePage::empty()))
        } else {
            (None, None)
        };
        (
            Some(KeyPushes {
                surface_id: focus.surface_id,
                commit: Some(echo.commit),
                action: None,
                preedit,
                candidates,
            }),
            echo,
        )
    }

    /// One editing command for the focused field. `None` = nothing to deliver: no text field
    /// holds focus (a window without one hears Escape only), or copy/cut from a password
    /// field. A running composition is committed first (Enter through the engine; its own
    /// pass-through is replaced by the command), a pending dead key is dropped.
    pub(crate) fn edit(&mut self, action: u8) -> Option<KeyPushes> {
        let focus = self.focus.filter(|f| f.field_kind != wire::FIELD_KIND_NONE)?;
        if self.password_focused() && matches!(action, wire::ACTION_COPY | wire::ACTION_CUT) {
            return None;
        }
        let composed = if self.strip_dirty {
            let outcome = self.engine.feed(ImeKey::Action(ImeAction::Enter));
            self.plan(&outcome).0
        } else {
            self.engine.reset();
            None
        };
        let mut pushes = composed.unwrap_or(KeyPushes {
            surface_id: focus.surface_id,
            commit: None,
            action: None,
            preedit: None,
            candidates: None,
        });
        pushes.action = Some(action);
        Some(pushes)
    }
}
