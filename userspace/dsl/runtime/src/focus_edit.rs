// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The focused field's undo and redo, its paste, and the pointer's caret (TASK-0346 P2). The
//! history (`nexus_textedit::History`) belongs to the focus session — another field starts a
//! fresh one; the same field re-tapped keeps it. The pointer's index is the host's: it knows the
//! glyph metrics the field was painted with, so a click lands where the person sees the char.

use crate::emit::Damage;
use crate::focus::EditResult;
use crate::interact::HandlerAction;
use crate::view::View;
use crate::{DeviceEnv, EffectHost, LocaleSource, RtError};
use nexus_dsl_ir::ui_ir_capnp::BindValue;
use nexus_textedit::{Change, Edit, History};
use nexus_theme_tokens::Tokens;

impl View<'_> {
    /// The focused field's history — a fresh one when the focus session changed.
    pub(crate) fn field_history(&mut self, session: u32) -> &mut History {
        if self.history_session != session {
            self.text_history = History::new();
            self.history_session = session;
        }
        &mut self.text_history
    }

    /// Pastes `text` at the caret, replacing the selection — one undo step of its own.
    ///
    /// # Errors
    /// Runtime errors from the write/emission.
    pub fn paste_text(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        text: &str,
    ) -> Result<Option<Damage>, RtError> {
        self.insert_as(tokens, device, locale, host, text, Change::Other)
    }

    /// Undoes the focused field's last change (a run of typing at once).
    ///
    /// # Errors
    /// Runtime errors from the write/emission.
    pub fn undo_text(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
    ) -> Result<EditResult, RtError> {
        self.travel(tokens, device, locale, host, true)
    }

    /// Redoes what the last undo took back.
    ///
    /// # Errors
    /// Runtime errors from the write/emission.
    pub fn redo_text(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
    ) -> Result<EditResult, RtError> {
        self.travel(tokens, device, locale, host, false)
    }

    fn travel(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        back: bool,
    ) -> Result<EditResult, RtError> {
        let Some(focused) = self.focused_text.clone() else {
            return Ok(EditResult::None);
        };
        let value = self.bound_text(focused.store, focused.instance, &focused.path);
        let history = self.field_history(focused.session);
        let step = if back {
            history.undo(&value, focused.edit)
        } else {
            history.redo(&value, focused.edit)
        };
        let Some(snapshot) = step else {
            return Ok(EditResult::None);
        };
        let applied = nexus_textedit::Applied {
            value: (snapshot.value != value).then_some(snapshot.value),
            edit: snapshot.edit,
        };
        self.commit_edit(tokens, device, locale, host, &focused, applied)
    }

    /// The pointer at char `idx` of the focused field: a click places the caret there; Shift+click
    /// and a drag (`extend`) move the caret there and keep the selection's other end.
    pub fn place_caret(&mut self, idx: usize, extend: bool) -> EditResult {
        let Some(focused) = self.focused_text.as_ref() else {
            return EditResult::None;
        };
        let value = self.bound_text(focused.store, focused.instance, &focused.path);
        let edit = nexus_textedit::place(&value, focused.edit, idx, extend);
        self.set_edit(edit)
    }

    /// A multi-click at char `idx` of the focused field: two clicks take the word, three
    /// everything (one places the caret).
    pub fn select_text_at(&mut self, idx: usize, clicks: u8) -> EditResult {
        let Some(focused) = self.focused_text.as_ref() else {
            return EditResult::None;
        };
        let value = self.bound_text(focused.store, focused.instance, &focused.path);
        self.set_edit(nexus_textedit::select_at(&value, idx, clicks))
    }

    /// Enter in the focused field: the innermost `on Submit` handler on it or around it runs —
    /// a search's, a form's, a composer's (TASK-0346 P2). `None` without a focus or a handler.
    ///
    /// # Errors
    /// Runtime errors from the dispatch.
    pub fn submit_text(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
    ) -> Result<Option<Damage>, RtError> {
        let Some(box_id) = self.focused_text.as_ref().map(|f| f.box_id) else {
            return Ok(None);
        };
        let sym = |name: &str| self.runtime.symbols().iter().position(|s| s == name);
        let (Some(submit), Some(change)) = (sym("Submit"), sym("Change")) else {
            return Ok(None);
        };
        let (submit, change) = (submit as u32, change as u32);
        let Some(field_path) = self
            .handlers
            .iter()
            .find(|(b, e)| *b == box_id && e.trigger == change)
            .map(|(_, e)| e.path.clone())
        else {
            return Ok(None);
        };
        // The innermost: the handler whose node path is the longest prefix of the field's own.
        let handler = self
            .handlers
            .iter()
            .filter(|(_, e)| {
                e.trigger == submit
                    && e.path.len() <= field_path.len()
                    && field_path[..e.path.len()] == e.path[..]
            })
            .max_by_key(|(_, e)| e.path.len())
            .map(|(_, e)| (e.instance, e.action.clone()));
        match handler {
            Some((instance, HandlerAction::Dispatch { event, case, payload })) => self
                .dispatch_in(instance, tokens, device, locale, host, event, case, payload)
                .map(Some),
            Some((_, HandlerAction::Navigate { path })) => {
                self.navigate(tokens, device, locale, &path).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// Tab (or Shift+Tab, `backward`): the text field after (before) the focused one in the
    /// page's order — reachable under the topmost modal, wrapping — as its handler box, for the
    /// host to focus like a tap. With no field focused, the first (the last). `None` when the
    /// page holds no other field.
    #[must_use]
    pub fn next_text_field(&self, backward: bool) -> Option<usize> {
        let change = self.runtime.symbols().iter().position(|s| s == "Change")? as u32;
        let confine = self.modal_confine();
        let mut fields: alloc::vec::Vec<usize> = self
            .handlers
            .iter()
            .filter(|(_, e)| {
                e.trigger == change
                    && matches!(e.action, HandlerAction::Bind { value: BindValue::Text, .. })
                    && crate::overlay::reachable(&e.path, confine)
            })
            .map(|(b, _)| *b)
            .collect();
        fields.sort_unstable();
        fields.dedup();
        let current = self.focused_text.as_ref().map(|f| f.box_id);
        let at = current.and_then(|c| fields.iter().position(|b| *b == c));
        let n = fields.len();
        if n == 0 || (n == 1 && at.is_some()) {
            return None;
        }
        let next = match (at, backward) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        fields.get(next).copied()
    }

    /// A caret or selection the pointer chose: repaint the field, end a typing run.
    fn set_edit(&mut self, edit: Edit) -> EditResult {
        let Some(focused) = self.focused_text.as_mut() else {
            return EditResult::None;
        };
        if focused.edit == edit {
            return EditResult::None;
        }
        focused.edit = edit;
        let session = focused.session;
        self.field_history(session).moved();
        EditResult::Caret
    }
}
