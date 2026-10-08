// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp`: the focused field's editing commands (TASK-0067B, RFC-0075
//! amendment). imed passes the keyboard's editing commands through composition; here they
//! become the runtime's caret/selection edits (`View::edit_text`) or clipboard calls: copy
//! and cut write the selection to clipboardd over this app's `CLIPBOARD` route, paste reads
//! the newest item (clipboardd allows it because this app's window has focus) and inserts it
//! at the caret. A password field never yields its text (`View::selected_text`). Markers
//! never carry text, and each fires once per process (like `apphost: text commit applied`):
//! a line per copy would log when the user copies.

use super::super::*;
use nexus_dsl_runtime::{Damage, EditCommand, EditResult};
use nexus_wire::imed as ime;

/// The re-read's damage joined with the edit's result: the larger wins, so one relayout —
/// after the last use of the frame's locale — covers both.
fn with_reread(changed: Option<Damage>, edit: EditResult) -> EditResult {
    match (changed, edit) {
        (Some(Damage::Layout), _) => EditResult::Value(Damage::Layout),
        (_, EditResult::Value(d)) => EditResult::Value(d),
        (Some(Damage::Paint), _) => EditResult::Value(Damage::Paint),
        (_, other) => other,
    }
}

/// The runtime command of an imed editing action (`None` = a clipboard action or unknown).
fn command_of(action: u8) -> Option<EditCommand> {
    Some(match action {
        ime::ACTION_LEFT => EditCommand::Left,
        ime::ACTION_RIGHT => EditCommand::Right,
        ime::ACTION_HOME => EditCommand::Home,
        ime::ACTION_END => EditCommand::End,
        ime::ACTION_SELECT_LEFT => EditCommand::SelectLeft,
        ime::ACTION_SELECT_RIGHT => EditCommand::SelectRight,
        ime::ACTION_SELECT_HOME => EditCommand::SelectHome,
        ime::ACTION_SELECT_END => EditCommand::SelectEnd,
        ime::ACTION_SELECT_ALL => EditCommand::SelectAll,
        ime::ACTION_BACKSPACE => EditCommand::Backspace,
        ime::ACTION_DELETE => EditCommand::Delete,
        _ => return None,
    })
}

impl DslApp {
    /// One editing action on the focused field; returns re-render need.
    pub(crate) fn edit_action(&mut self, action: u8) -> bool {
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let locale = super::super::app_locale!(self);
        let result = match (command_of(action), action) {
            (Some(cmd), _) => self.view.edit_text(tokens, &device, &locale, &mut self.host, cmd),
            (None, ime::ACTION_COPY | ime::ACTION_CUT) => {
                let Some(text) = self.view.selected_text() else {
                    return false;
                };
                if !self.host.clipboard_copy(&text) {
                    raw_marker("apphost: text copy FAIL");
                    return false;
                }
                if crate::proof_line::TEXT_COPY.claim() {
                    raw_marker("apphost: text copy ok");
                }
                // A history this page shows re-reads at once (`on ClipboardChanged`).
                let changed = self
                    .view
                    .fire_trigger(tokens, &device, &locale, &mut self.host, "ClipboardChanged")
                    .ok()
                    .flatten();
                // A copy leaves the field alone, and so does a cut the clipboard could not
                // hold whole (over the item bound it stored the head — removing the rest
                // would lose text): then only the re-read repaints.
                let whole = nexus_wire::clipboardd::item_text(&text).len() == text.len();
                let edit = if action == ime::ACTION_CUT && whole {
                    self.view.cut_selection(tokens, &device, &locale, &mut self.host)
                } else {
                    Ok(EditResult::None)
                };
                edit.map(|e| with_reread(changed, e))
            }
            (None, ime::ACTION_PASTE) => {
                let Some(text) = self.host.clipboard_paste_text() else {
                    return false;
                };
                if crate::proof_line::TEXT_PASTE.claim() {
                    raw_marker("apphost: text paste ok");
                }
                self.view
                    .insert_text(tokens, &device, &locale, &mut self.host, &text)
                    .map(|d| d.map_or(EditResult::None, EditResult::Value))
            }
            _ => return false,
        };
        match result {
            Ok(EditResult::Value(Damage::Layout)) => {
                self.relayout_retained();
                true
            }
            Ok(EditResult::Value(_) | EditResult::Caret) => true,
            Ok(EditResult::None) => false,
            Err(e) => {
                raw_marker(&alloc::format!("apphost: text action ERR {e:?}"));
                false
            }
        }
    }
}
