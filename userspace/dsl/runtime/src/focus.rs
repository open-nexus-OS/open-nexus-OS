// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The focused-field model (RFC-0075): tap-to-focus on Change-bound text
//! fields, focused insert/backspace editing, and re-emit-safe focus identity.
//! Widget focus is APP authority — the host announces transitions upward
//! (`OP_SURFACE_TEXT_FOCUS`) and delivers composed text back down into the
//! focused field; the runtime never sees raw key codes.

use crate::emit::Damage;
use crate::interact::{self, HandlerAction, ScrollView};
use crate::store::Value;
use crate::view::View;
use crate::{DeviceEnv, EffectHost, LocaleSource, RtError};
use alloc::vec::Vec;
use nexus_dsl_ir::ui_ir_capnp::BindValue;
use nexus_layout_types::LayoutNode;
use nexus_theme_tokens::Tokens;

/// Snapshot of the focused text field for the host (RFC-0075): the box id
/// resolves the caret-anchor rect in the current layout; `secure` fields get
/// no IME preview/candidates/learning downstream. The caret and the selection
/// (TASK-0067B) are CHAR indices into the field's value — the host paints them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFocusSnapshot {
    /// Pre-order box id of the field's handler node (`LayoutBox::node_id`).
    pub box_id: usize,
    /// Password field (from the widget's `secure` prop).
    pub secure: bool,
    /// The caret: before char `caret` of the value.
    pub caret: usize,
    /// The selected char range `[start, end)`, if any.
    pub selection: Option<(usize, usize)>,
}

/// What an editing command did (TASK-0067B).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditResult {
    /// Nothing (no focus, or the command changed nothing).
    None,
    /// Only the caret or the selection moved — repaint the field, no re-emit.
    Caret,
    /// The value changed — the re-emit's damage.
    Value(Damage),
}

/// The focused field's binding target (survives re-emits by identity, not id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FocusedText {
    pub(crate) store: u32,
    /// The instance the focused field's store belongs to (TASK-0077B P1).
    pub(crate) instance: u64,
    pub(crate) path: Vec<u32>,
    pub(crate) box_id: usize,
    pub(crate) secure: bool,
    /// Caret + selection (TASK-0067B), char indices into the value.
    pub(crate) edit: nexus_textedit::Edit,
    /// The enclosing `on Change -> dispatch(E)` handler, resolved ONCE per
    /// focus. Typing writes the binding; without this it did nothing else, so
    /// `on Change` was dead on every keystroke in every app — a store field
    /// moved and no effect ever ran (see [`View::insert_text`]).
    pub(crate) change_dispatch: Option<ChangeDispatch>,
}

/// The `(event, case, payload)` of a Change handler that DISPATCHES, as
/// opposed to the field's own two-way `Bind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangeDispatch {
    pub(crate) event: u32,
    pub(crate) case: u32,
    pub(crate) payload: Vec<Value>,
}

/// Upper bound for text-field values written through the focus path
/// (bounded state, RFC-0075 — fields are UI inputs, not documents).
const TEXT_VALUE_MAX_CHARS: usize = 256;

impl View<'_> {
    /// Tap-to-focus (RFC-0075): focuses the innermost Change-bound text field
    /// under (x, y) and returns its snapshot; a tap that hits no field CLEARS
    /// the focus and returns `None` — unless the focused field is an
    /// `.autofocus(true)` field still on screen, which keeps the keyboard (see
    /// [`Self::autofocus_holds`]). The host announces every transition upward
    /// (`OP_SURFACE_TEXT_FOCUS`) — widget focus is app authority.
    #[must_use]
    pub fn focus_text_at(
        &mut self,
        boxes: &[nexus_layout::LayoutBox],
        x: nexus_layout_types::FxPx,
        y: nexus_layout_types::FxPx,
        scroll: Option<ScrollView>,
    ) -> Option<TextFocusSnapshot> {
        let trigger_sym =
            self.runtime.symbols().iter().position(|s| s == "Change").map(|i| i as u32);
        // Confined like every pointer hit: a field behind the open modal
        // cannot take focus (TASK-0074 focus trap).
        let confine = self.modal_confine();
        let hit = trigger_sym.and_then(|sym| {
            interact::hit_scrolled(&self.handlers, boxes, sym, x, y, scroll, confine)
        });
        match hit {
            Some(hit) => {
                let (box_id, entry) = (hit.box_id, hit.entry);
                let node_path = entry.path.clone();
                // Only a bind that TAKES TEXT is a text field. The trigger
                // alone is not enough: it says when, the rule says what
                // (IR v1.8) — and handing the IME a control that cannot
                // accept a string is how a keyboard opens over nothing.
                let HandlerAction::Bind { store, path, value: BindValue::Text } =
                    entry.action.clone()
                else {
                    return self.keep_or_clear_focus();
                };
                let secure = subtree_is_secure(&self.scene, box_id);
                let change_dispatch =
                    trigger_sym.and_then(|sym| self.enclosing_change_dispatch(sym, &node_path));
                // A tap focuses with the caret at the end (pointer placement is TASK-0095's).
                let len = self.bound_text(store, entry.instance, &path).chars().count();
                let edit = nexus_textedit::Edit::at_end(len);
                self.focused_text = Some(FocusedText {
                    store,
                    instance: entry.instance,
                    path,
                    box_id,
                    secure,
                    edit,
                    change_dispatch,
                });
                Some(TextFocusSnapshot { box_id, secure, caret: edit.caret, selection: None })
            }
            None => self.keep_or_clear_focus(),
        }
    }

    /// The innermost `Change` handler ABOVE `node_path` whose action is a
    /// dispatch — i.e. the app's `Stack { TextField { … } } on Change -> …`
    /// wrapper, which is how every page in the tree writes it.
    ///
    /// Resolved by handler PATH PREFIX, not by geometry: an ancestor's
    /// child-index path is a proper prefix of its descendant's. That works
    /// identically before layout exists, which is what lets
    /// [`View::revalidate_text_focus`] re-resolve it during a re-emit.
    fn enclosing_change_dispatch(
        &self,
        trigger_sym: u32,
        node_path: &[u32],
    ) -> Option<ChangeDispatch> {
        let mut best: Option<(usize, ChangeDispatch)> = None;
        for (_, entry) in &self.handlers {
            if entry.trigger != trigger_sym || entry.path.len() >= node_path.len() {
                continue;
            }
            if node_path[..entry.path.len()] != entry.path[..] {
                continue;
            }
            let HandlerAction::Dispatch { event, case, payload } = &entry.action else {
                continue;
            };
            // Longest prefix = innermost enclosing handler.
            if best.as_ref().is_none_or(|(len, _)| entry.path.len() > *len) {
                best = Some((
                    entry.path.len(),
                    ChangeDispatch { event: *event, case: *case, payload: payload.clone() },
                ));
            }
        }
        best.map(|(_, d)| d)
    }

    /// A tap that hit no text field: the focus stays with a holding autofocus field and
    /// is cleared otherwise.
    fn keep_or_clear_focus(&mut self) -> Option<TextFocusSnapshot> {
        if self.autofocus_holds() {
            return self.text_focus();
        }
        self.focused_text = None;
        None
    }

    /// `.autofocus(true)` holds the keyboard while its field is reachable: a press on a
    /// control beside it (the search's category buttons) leaves the focus where it is.
    /// Clearing it would hand it back one present later through [`Self::autofocus_box`] —
    /// and a key typed in that gap would land nowhere (TASK-0067B board round). A press on
    /// another field still moves the focus; a press that closes the layer drops the field,
    /// and revalidation clears the focus with it.
    fn autofocus_holds(&self) -> bool {
        let Some(focused) = self.focused_text.as_ref() else {
            return false;
        };
        let confine = self.modal_confine();
        self.handlers.iter().any(|(box_id, entry)| {
            *box_id == focused.box_id
                && entry.autofocus
                && crate::overlay::reachable(&entry.path, confine)
        })
    }

    /// The text field that should take focus now (`.autofocus(true)`, TASK-0067B): the
    /// first autofocus text-field bind reachable under the topmost modal, while NO field
    /// holds focus. Returns its pre-order box id; the host focuses it through the tap path
    /// (`focus_text_at` at the box centre) and announces the transition like a tap, so
    /// one code path owns focus whichever way it arrives.
    #[must_use]
    pub fn autofocus_box(&self) -> Option<usize> {
        if self.focused_text.is_some() {
            return None;
        }
        let confine = self.modal_confine();
        self.handlers
            .iter()
            .find(|(_, entry)| {
                entry.autofocus
                    && matches!(entry.action, HandlerAction::Bind { value: BindValue::Text, .. })
                    && crate::overlay::reachable(&entry.path, confine)
            })
            .map(|(box_id, _)| *box_id)
    }

    /// The current text focus, if any.
    #[must_use]
    pub fn text_focus(&self) -> Option<TextFocusSnapshot> {
        self.focused_text.as_ref().map(|f| TextFocusSnapshot {
            box_id: f.box_id,
            secure: f.secure,
            caret: f.edit.caret,
            selection: f.edit.selection(),
        })
    }

    /// Clears the text focus (surface focus loss, Escape) — the host
    /// announces the transition and the composer flushes upstream.
    pub fn clear_text_focus(&mut self) {
        self.focused_text = None;
    }

    /// Inserts committed text into the FOCUSED field (append-at-end, v1 caret
    /// model). No-op without focus. Values are bounded (`TEXT_VALUE_MAX_CHARS`).
    ///
    /// # Errors
    /// Runtime errors from the write/emission.
    pub fn insert_text(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        text: &str,
    ) -> Result<Option<Damage>, RtError> {
        let Some(focused) = self.focused_text.clone() else {
            return Ok(None);
        };
        let value = self.bound_text(focused.store, focused.instance, &focused.path);
        let applied = nexus_textedit::insert(&value, focused.edit, text, TEXT_VALUE_MAX_CHARS);
        self.commit_edit(tokens, device, locale, host, &focused, applied).map(|r| match r {
            EditResult::Value(d) => Some(d),
            _ => None,
        })
    }

    /// Deletes the last character of the FOCUSED field. No-op without focus
    /// or on an empty value.
    ///
    /// # Errors
    /// Runtime errors from the write/emission.
    pub fn backspace_text(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
    ) -> Result<Option<Damage>, RtError> {
        match self.edit_text(tokens, device, locale, host, nexus_textedit::Command::Backspace)? {
            EditResult::Value(d) => Ok(Some(d)),
            _ => Ok(None),
        }
    }

    /// One editing command on the FOCUSED field (TASK-0067B): caret moves, Shift-selection,
    /// select-all, backspace/delete at the caret or over the selection.
    ///
    /// # Errors
    /// Runtime errors from the write/emission.
    pub fn edit_text(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        cmd: nexus_textedit::Command,
    ) -> Result<EditResult, RtError> {
        let Some(focused) = self.focused_text.clone() else {
            return Ok(EditResult::None);
        };
        let value = self.bound_text(focused.store, focused.instance, &focused.path);
        let applied = nexus_textedit::apply(&value, focused.edit, cmd);
        self.commit_edit(tokens, device, locale, host, &focused, applied)
    }

    /// The focused field's selected text — `None` without a selection, and ALWAYS `None` for
    /// a password field (nothing is ever copied out of one).
    #[must_use]
    pub fn selected_text(&self) -> Option<alloc::string::String> {
        let focused = self.focused_text.as_ref().filter(|f| !f.secure)?;
        let value = self.bound_text(focused.store, focused.instance, &focused.path);
        nexus_textedit::selected_text(&value, focused.edit)
    }

    /// Removes the focused field's selection (the cut) — never in a password field.
    ///
    /// # Errors
    /// Runtime errors from the write/emission.
    pub fn cut_selection(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
    ) -> Result<EditResult, RtError> {
        let Some(focused) = self.focused_text.clone().filter(|f| !f.secure) else {
            return Ok(EditResult::None);
        };
        let value = self.bound_text(focused.store, focused.instance, &focused.path);
        let applied = nexus_textedit::delete_selection(&value, focused.edit);
        self.commit_edit(tokens, device, locale, host, &focused, applied)
    }

    /// The bound value of a text field ("" when unset or not a string).
    fn bound_text(&self, store: u32, instance: u64, path: &[u32]) -> alloc::string::String {
        match self.runtime.read_binding(store, instance, path) {
            Some(Value::Str(s)) => s.clone(),
            _ => alloc::string::String::new(),
        }
    }

    /// Applies an engine result: writes a changed value (with the field's `on Change`), then
    /// records the caret — AFTER the write, whose re-emit re-anchors the focus.
    fn commit_edit(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        focused: &FocusedText,
        applied: nexus_textedit::Applied,
    ) -> Result<EditResult, RtError> {
        let result = match applied.value {
            Some(value) => {
                EditResult::Value(self.write_focused(tokens, device, locale, host, focused, value)?)
            }
            None if applied.edit != focused.edit => EditResult::Caret,
            None => EditResult::None,
        };
        if let Some(f) = self.focused_text.as_mut() {
            f.edit = applied.edit;
        }
        Ok(result)
    }

    /// Writes the focused field AND runs its enclosing `on Change ->
    /// dispatch(E)`, if the page declared one.
    ///
    /// Without the dispatch a keystroke only moved a store field: no reducer
    /// arm, no `@effect`. That is why live search never worked anywhere —
    /// every page in the tree writes the pattern
    /// `Stack { TextField { value: $state.q } } on Change -> dispatch(E)`, and
    /// `Change` was consulted only to resolve focus and pick the I-beam
    /// cursor. Worse, an event that nothing dispatches becomes a ROOT effect
    /// (`initial::root_effect_events`), so the app's own filter ran once at
    /// mount and never again.
    ///
    /// Both change sets are applied in ONE `apply_changes`, so a keystroke
    /// still costs exactly one re-emit.
    fn write_focused(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        focused: &FocusedText,
        value: alloc::string::String,
    ) -> Result<Damage, RtError> {
        let mut changes = self.runtime.write_binding(
            focused.store,
            focused.instance,
            &focused.path,
            Value::Str(value),
        )?;
        if let Some(d) = &focused.change_dispatch {
            let more =
                self.runtime.dispatch(device, locale, host, d.event, d.case, d.payload.clone())?;
            changes.extend(more);
        }
        self.apply_changes(tokens, device, locale, &changes)
    }

    /// Re-anchors the text focus after a re-emit: focus survives by binding
    /// identity (store, path), not box id — the field keeps focus while its
    /// Change handler exists in the new scene; a page switch or removed
    /// field drops it. Called by `View::emit`.
    pub(crate) fn revalidate_text_focus(&mut self) {
        let Some(focused) = self.focused_text.take() else {
            return;
        };
        let change_sym =
            self.runtime.symbols().iter().position(|s| s == "Change").map(|i| i as u32);
        let Some(sym) = change_sym else {
            return;
        };
        // Re-anchor by binding identity, then re-resolve the enclosing
        // dispatch: handler paths are stable across a re-emit but box ids are
        // not, and the wrapper may have moved into a different `if` arm.
        let found = self.handlers.iter().find_map(|(box_id, entry)| {
            let HandlerAction::Bind { store, path, value: BindValue::Text } = &entry.action else {
                return None;
            };
            (entry.trigger == sym && *store == focused.store && *path == focused.path)
                .then(|| (*box_id, entry.path.clone()))
        });
        // The caret survives the re-emit, clamped: a reducer may have shortened the value.
        let len = self.bound_text(focused.store, focused.instance, &focused.path).chars().count();
        self.focused_text = found.map(|(box_id, node_path)| FocusedText {
            store: focused.store,
            instance: focused.instance,
            path: focused.path.clone(),
            box_id,
            secure: subtree_is_secure(&self.scene, box_id),
            edit: focused.edit.clamped(len),
            change_dispatch: self.enclosing_change_dispatch(sym, &node_path),
        });
    }
}

/// Whether the pre-order node `box_id` contains a `secure` TextInput — the
/// password signal for the focus snapshot (the Change handler sits on the
/// widget's root node; the input node lives in its subtree).
fn subtree_is_secure(scene: &LayoutNode, box_id: usize) -> bool {
    fn walk(node: &LayoutNode, next_id: &mut usize, target: usize) -> Option<bool> {
        let id = *next_id;
        *next_id += 1;
        let inside = id == target;
        match node {
            LayoutNode::TextInput(input, _) => inside.then_some(input.secure),
            LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => {
                if inside {
                    Some(any_secure_children(children))
                } else {
                    children.iter().find_map(|c| walk(c, next_id, target))
                }
            }
            LayoutNode::Spacer(_) | LayoutNode::Text(_, _) => None,
        }
    }
    fn any_secure_children(children: &[LayoutNode]) -> bool {
        children.iter().any(|node| match node {
            LayoutNode::TextInput(input, _) => input.secure,
            LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => {
                any_secure_children(children)
            }
            LayoutNode::Spacer(_) | LayoutNode::Text(_, _) => false,
        })
    }
    let mut next_id = 1usize;
    walk(scene, &mut next_id, box_id).unwrap_or(false)
}
