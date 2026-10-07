// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the caret + selection engine of a single-line text field (TASK-0095's
//! `textedit_core`, built for TASK-0067B — copy must work). Pure and `no_std`: a field's
//! VALUE lives in its app's store; this crate only knows where the caret is and what is
//! selected, as CHARACTER indices into that value, and turns one command or one insert into
//! the next value and the next caret. The DSL runtime keeps one [`Edit`] per focused field;
//! the host paints the caret and the selection from it.
//!
//! Selection = the range between the `anchor` (where Shift-movement began) and the `caret`;
//! no anchor, or an anchor equal to the caret, means nothing is selected. Movement without
//! Shift collapses a selection to its edge, as text fields do on every desktop.
//!
//! Not here yet (TASK-0095): word jumps, pointer drag and double/triple-click selection,
//! grapheme clusters (a char is a Unicode scalar), bidi.
//! OWNERS: @ui
//! STATUS: Functional (keyboard core)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;

/// Where the caret is and where a selection began, as char indices into the value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edit {
    /// The caret: before char `caret` (0 = start, `len` = end).
    pub caret: usize,
    /// Where the selection began, if one is being made.
    pub anchor: Option<usize>,
}

impl Edit {
    /// The caret at the end of a value of `len` chars, nothing selected (a fresh focus).
    #[must_use]
    pub const fn at_end(len: usize) -> Self {
        Self { caret: len, anchor: None }
    }

    /// Keeps the caret and the anchor inside a value of `len` chars (the value can change
    /// under the field — a reducer clears it, an effect fills it).
    #[must_use]
    pub fn clamped(self, len: usize) -> Self {
        Self { caret: self.caret.min(len), anchor: self.anchor.map(|a| a.min(len)) }
    }

    /// The selected range `[start, end)`, ordered; `None` when nothing is selected.
    #[must_use]
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.caret).then(|| (anchor.min(self.caret), anchor.max(self.caret)))
    }
}

/// One editing command (the keyboard's — arrows, Home/End, Shift for selection, Ctrl+A,
/// Backspace, Delete). Copy, cut and paste are the host's: they read [`selected_text`] and
/// call [`delete_selection`] / [`insert`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Left,
    Right,
    Home,
    End,
    SelectLeft,
    SelectRight,
    SelectHome,
    SelectEnd,
    SelectAll,
    Backspace,
    Delete,
}

/// The result of an edit: the new value when it changed, and the next caret state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// `Some` only when the value changed (a pure caret move leaves it `None`).
    pub value: Option<String>,
    /// The caret and anchor after the edit.
    pub edit: Edit,
}

fn char_len(value: &str) -> usize {
    value.chars().count()
}

/// Byte offset of char index `idx` (clamped to the end).
#[must_use]
pub fn byte_at(value: &str, idx: usize) -> usize {
    value.char_indices().nth(idx).map_or(value.len(), |(b, _)| b)
}

/// The value with the char range `[start, end)` replaced by `with`.
fn splice(value: &str, start: usize, end: usize, with: &str) -> String {
    let (b0, b1) = (byte_at(value, start), byte_at(value, end));
    let mut out = String::with_capacity(value.len() - (b1 - b0) + with.len());
    out.push_str(&value[..b0]);
    out.push_str(with);
    out.push_str(&value[b1..]);
    out
}

/// Applies one command to `value` at `edit`.
#[must_use]
pub fn apply(value: &str, edit: Edit, cmd: Command) -> Applied {
    let len = char_len(value);
    let edit = edit.clamped(len);
    let moved = |caret: usize| Applied { value: None, edit: Edit { caret, anchor: None } };
    let extend = |caret: usize| Applied {
        value: None,
        edit: Edit { caret, anchor: Some(edit.anchor.unwrap_or(edit.caret)) },
    };
    match cmd {
        // Without Shift a selection collapses to the edge in the direction of travel.
        Command::Left => match edit.selection() {
            Some((start, _)) => moved(start),
            None => moved(edit.caret.saturating_sub(1)),
        },
        Command::Right => match edit.selection() {
            Some((_, end)) => moved(end),
            None => moved((edit.caret + 1).min(len)),
        },
        Command::Home => moved(0),
        Command::End => moved(len),
        Command::SelectLeft => extend(edit.caret.saturating_sub(1)),
        Command::SelectRight => extend((edit.caret + 1).min(len)),
        Command::SelectHome => extend(0),
        Command::SelectEnd => extend(len),
        Command::SelectAll => Applied { value: None, edit: Edit { caret: len, anchor: Some(0) } },
        Command::Backspace => match edit.selection() {
            Some(_) => delete_selection(value, edit),
            None if edit.caret == 0 => moved(0),
            None => Applied {
                value: Some(splice(value, edit.caret - 1, edit.caret, "")),
                edit: Edit { caret: edit.caret - 1, anchor: None },
            },
        },
        Command::Delete => match edit.selection() {
            Some(_) => delete_selection(value, edit),
            None if edit.caret >= len => moved(len),
            None => Applied {
                value: Some(splice(value, edit.caret, edit.caret + 1, "")),
                edit: Edit { caret: edit.caret, anchor: None },
            },
        },
    }
}

/// Inserts `text` at the caret, replacing the selection, keeping the value at most
/// `max_chars` long (the inserted text is cut, never the existing value).
#[must_use]
pub fn insert(value: &str, edit: Edit, text: &str, max_chars: usize) -> Applied {
    let len = char_len(value);
    let edit = edit.clamped(len);
    let (start, end) = edit.selection().unwrap_or((edit.caret, edit.caret));
    let room = max_chars.saturating_sub(len - (end - start));
    let take: String = text.chars().take(room).collect();
    let added = char_len(&take);
    if added == 0 && start == end {
        return Applied { value: None, edit };
    }
    Applied {
        value: Some(splice(value, start, end, &take)),
        edit: Edit { caret: start + added, anchor: None },
    }
}

/// The selected text, if anything is selected.
#[must_use]
pub fn selected_text(value: &str, edit: Edit) -> Option<String> {
    let (start, end) = edit.clamped(char_len(value)).selection()?;
    Some(String::from(&value[byte_at(value, start)..byte_at(value, end)]))
}

/// Removes the selection (the cut); the caret lands where it began.
#[must_use]
pub fn delete_selection(value: &str, edit: Edit) -> Applied {
    let edit = edit.clamped(char_len(value));
    match edit.selection() {
        Some((start, end)) => Applied {
            value: Some(splice(value, start, end, "")),
            edit: Edit { caret: start, anchor: None },
        },
        None => Applied { value: None, edit },
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::ToString;

    fn at(caret: usize) -> Edit {
        Edit { caret, anchor: None }
    }

    #[test]
    fn arrows_move_the_caret_and_stop_at_the_ends() {
        let v = "abc";
        assert_eq!(apply(v, at(3), Command::Left).edit, at(2));
        assert_eq!(apply(v, at(0), Command::Left).edit, at(0));
        assert_eq!(apply(v, at(3), Command::Right).edit, at(3));
        assert_eq!(apply(v, at(1), Command::Home).edit, at(0));
        assert_eq!(apply(v, at(1), Command::End).edit, at(3));
        assert_eq!(apply(v, at(1), Command::Right).value, None, "a move changes no value");
    }

    #[test]
    fn shift_extends_and_a_plain_arrow_collapses_to_the_edge() {
        let v = "Hallo Welt";
        let e = apply(v, at(10), Command::SelectLeft).edit;
        let e = apply(v, e, Command::SelectLeft).edit;
        assert_eq!(e.selection(), Some((8, 10)));
        assert_eq!(selected_text(v, e).as_deref(), Some("lt"));
        assert_eq!(apply(v, e, Command::Left).edit, at(8), "left collapses to the start");
        assert_eq!(apply(v, e, Command::Right).edit, at(10), "right collapses to the end");
        let e = apply(v, at(5), Command::SelectHome).edit;
        assert_eq!(selected_text(v, e).as_deref(), Some("Hallo"));
        let back = apply(v, apply(v, at(3), Command::SelectRight).edit, Command::SelectLeft).edit;
        assert_eq!(back.selection(), None, "back at the anchor = nothing selected");
    }

    #[test]
    fn select_all_then_type_replaces_everything() {
        let v = "alt";
        let e = apply(v, at(1), Command::SelectAll).edit;
        assert_eq!(selected_text(v, e).as_deref(), Some("alt"));
        let a = insert(v, e, "neu", 256);
        assert_eq!(a.value.as_deref(), Some("neu"));
        assert_eq!(a.edit, at(3));
    }

    #[test]
    fn insert_lands_at_the_caret_and_is_bounded() {
        let a = insert("Wlt", at(1), "e", 256);
        assert_eq!((a.value.as_deref(), a.edit), (Some("Welt"), at(2)));
        let a = insert("abcd", at(4), "xyz", 5);
        assert_eq!(a.value.as_deref(), Some("abcdx"), "only what fits is inserted");
        assert_eq!(insert("abcde", at(5), "x", 5).value, None, "a full field takes nothing");
    }

    #[test]
    fn backspace_and_delete_respect_caret_and_selection() {
        let a = apply("abc", at(2), Command::Backspace);
        assert_eq!((a.value.as_deref(), a.edit), (Some("ac"), at(1)));
        let a = apply("abc", at(1), Command::Delete);
        assert_eq!((a.value.as_deref(), a.edit), (Some("ac"), at(1)));
        assert_eq!(apply("abc", at(0), Command::Backspace).value, None);
        assert_eq!(apply("abc", at(3), Command::Delete).value, None);
        let sel = Edit { caret: 1, anchor: Some(3) };
        let a = apply("abcd", sel, Command::Backspace);
        assert_eq!((a.value.as_deref(), a.edit), (Some("ad"), at(1)));
    }

    #[test]
    fn multibyte_chars_are_one_step_each() {
        let v = "Äpfel ü";
        let e = apply(v, at(7), Command::SelectLeft).edit;
        assert_eq!(selected_text(v, e).as_deref(), Some("ü"));
        let a = apply(v, at(1), Command::Backspace);
        assert_eq!(a.value.as_deref(), Some("pfel ü"));
        assert_eq!(byte_at(v, 1), 2, "Ä is two bytes");
    }

    #[test]
    fn cut_removes_the_selection_and_a_stale_caret_is_clamped() {
        let e = Edit { caret: 5, anchor: Some(0) };
        assert_eq!(selected_text("Hallo Welt", e).as_deref(), Some("Hallo"));
        let a = delete_selection("Hallo Welt", e);
        assert_eq!((a.value.map(|s| s.to_string()), a.edit), (Some(" Welt".to_string()), at(0)));
        // The value shrank under the field: the caret and anchor stay inside it.
        let stale = Edit { caret: 40, anchor: Some(30) };
        assert_eq!(stale.clamped(3), Edit { caret: 3, anchor: Some(3) });
        assert_eq!(apply("abc", stale, Command::Left).edit, at(2));
        assert_eq!(delete_selection("abc", at(1)).value, None, "nothing selected, nothing cut");
    }
}
