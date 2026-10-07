// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the keymap's text-field editing commands as imed wire action codes (TASK-0067B,
//! RFC-0075 amendment). inputd resolves the key (navigation keys, Ctrl+A/C/X/V by the
//! layout's letter) and forwards the command like Enter or Backspace; imed passes it to the
//! focused field. One table, so the two vocabularies cannot drift.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit test below

use keymaps::EditKey;
use nexus_wire::imed as wire;

/// The imed `ACTION_*` code of an editing command.
#[must_use]
pub fn action_code(key: EditKey) -> u8 {
    match key {
        EditKey::Left => wire::ACTION_LEFT,
        EditKey::Right => wire::ACTION_RIGHT,
        EditKey::Home => wire::ACTION_HOME,
        EditKey::End => wire::ACTION_END,
        EditKey::SelectLeft => wire::ACTION_SELECT_LEFT,
        EditKey::SelectRight => wire::ACTION_SELECT_RIGHT,
        EditKey::SelectHome => wire::ACTION_SELECT_HOME,
        EditKey::SelectEnd => wire::ACTION_SELECT_END,
        EditKey::Delete => wire::ACTION_DELETE,
        EditKey::SelectAll => wire::ACTION_SELECT_ALL,
        EditKey::Copy => wire::ACTION_COPY,
        EditKey::Cut => wire::ACTION_CUT,
        EditKey::Paste => wire::ACTION_PASTE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_editing_command_maps_to_a_distinct_edit_action() {
        let all = [
            EditKey::Left,
            EditKey::Right,
            EditKey::Home,
            EditKey::End,
            EditKey::SelectLeft,
            EditKey::SelectRight,
            EditKey::SelectHome,
            EditKey::SelectEnd,
            EditKey::Delete,
            EditKey::SelectAll,
            EditKey::Copy,
            EditKey::Cut,
            EditKey::Paste,
        ];
        let codes: Vec<u8> = all.iter().map(|k| action_code(*k)).collect();
        for (i, c) in codes.iter().enumerate() {
            assert!(wire::is_edit_action(*c), "{:?} is an edit action", all[i]);
            assert!(!codes[i + 1..].contains(c), "{:?} shares its code", all[i]);
        }
        assert!(
            !wire::is_edit_action(wire::ACTION_ENTER) && !wire::is_edit_action(wire::ACTION_TAB)
        );
    }
}
