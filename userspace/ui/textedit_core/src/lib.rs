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
//! TASK-0346 P2: word moves and selection, deletion by words (`words`), the pointer's caret —
//! a click places it, Shift+click and a drag extend, a double click takes the word, a triple
//! click everything ([`place`], [`select_at`]) — and undo/redo (`history`).
//! Not here: grapheme clusters (a char is a Unicode scalar), bidi, multi-line.
//! OWNERS: @ui
//! STATUS: Functional (keyboard + pointer core)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;

/// Undo and redo for one field.
pub mod history;
/// Where a field shows its text: the horizontal offset that keeps the caret in view.
pub mod view;
/// Word boundaries (the word commands, the double click).
pub mod words;

pub use history::{Change, History, Snapshot, HISTORY_MAX};
use words::{run_at, word_left, word_right};

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
/// Backspace, Delete, and with Ctrl the word steps). Copy, cut and paste are the host's: they
/// read [`selected_text`] and call [`delete_selection`] / [`insert`]; undo and redo are the
/// runtime's ([`History`]).
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
    /// Ctrl+Left / Ctrl+Right: to the start of the word before, the end of the word after.
    WordLeft,
    WordRight,
    /// Ctrl+Shift+Left / Ctrl+Shift+Right.
    SelectWordLeft,
    SelectWordRight,
    /// Ctrl+Backspace / Ctrl+Delete: the word before or after the caret (or the selection).
    DeleteWordBack,
    DeleteWordForward,
}

impl Command {
    /// Whether the command can change the value (an undo step), not only the caret.
    #[must_use]
    pub const fn deletes(self) -> bool {
        matches!(
            self,
            Self::Backspace | Self::Delete | Self::DeleteWordBack | Self::DeleteWordForward
        )
    }
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
        // A word step without Shift leaves from the selection's edge in its direction.
        Command::WordLeft => {
            moved(word_left(value, edit.selection().map_or(edit.caret, |(start, _)| start)))
        }
        Command::WordRight => {
            moved(word_right(value, edit.selection().map_or(edit.caret, |(_, end)| end)))
        }
        Command::SelectWordLeft => extend(word_left(value, edit.caret)),
        Command::SelectWordRight => extend(word_right(value, edit.caret)),
        Command::DeleteWordBack => match edit.selection() {
            Some(_) => delete_selection(value, edit),
            None => delete_range(value, word_left(value, edit.caret), edit.caret),
        },
        Command::DeleteWordForward => match edit.selection() {
            Some(_) => delete_selection(value, edit),
            None => delete_range(value, edit.caret, word_right(value, edit.caret)),
        },
    }
}

/// The char range `[start, end)` removed, the caret at `start` (nothing when it is empty).
fn delete_range(value: &str, start: usize, end: usize) -> Applied {
    if start >= end {
        return Applied { value: None, edit: Edit { caret: start, anchor: None } };
    }
    Applied {
        value: Some(splice(value, start, end, "")),
        edit: Edit { caret: start, anchor: None },
    }
}

/// The pointer at char index `idx`: a click places the caret there; Shift+click and a drag
/// (`extend`) move the caret there and keep the selection's other end.
#[must_use]
pub fn place(value: &str, edit: Edit, idx: usize, extend: bool) -> Edit {
    let len = char_len(value);
    let (edit, idx) = (edit.clamped(len), idx.min(len));
    if extend {
        Edit { caret: idx, anchor: Some(edit.anchor.unwrap_or(edit.caret)) }
    } else {
        Edit { caret: idx, anchor: None }
    }
}

/// A multi-click at char index `idx`: two clicks take the word (or the run of separators) under
/// it, three take everything; one click places the caret.
#[must_use]
pub fn select_at(value: &str, idx: usize, clicks: u8) -> Edit {
    let len = char_len(value);
    match clicks {
        0 | 1 => Edit { caret: idx.min(len), anchor: None },
        2 => {
            let (start, end) = run_at(value, idx.min(len));
            Edit { caret: end, anchor: Some(start) }
        }
        _ => Edit { caret: len, anchor: Some(0) },
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

#[cfg(test)]
mod pointer_and_word_tests {
    use super::*;

    fn at(caret: usize) -> Edit {
        Edit { caret, anchor: None }
    }

    #[test]
    fn word_steps_move_and_select_by_words() {
        let v = "eins zwei drei";
        assert_eq!(apply(v, at(14), Command::WordLeft).edit, at(10));
        assert_eq!(apply(v, at(0), Command::WordRight).edit, at(4));
        let e = apply(v, at(14), Command::SelectWordLeft).edit;
        let e = apply(v, e, Command::SelectWordLeft).edit;
        assert_eq!(selected_text(v, e).as_deref(), Some("zwei drei"));
        let sel = Edit { caret: 9, anchor: Some(5) };
        assert_eq!(apply(v, sel, Command::WordRight).edit, at(14), "leaves from the right edge");
    }

    #[test]
    fn word_deletion_takes_the_word_or_the_selection() {
        let a = apply("eins zwei drei", at(14), Command::DeleteWordBack);
        assert_eq!((a.value.as_deref(), a.edit), (Some("eins zwei "), at(10)));
        let a = apply("eins zwei drei", at(4), Command::DeleteWordForward);
        assert_eq!((a.value.as_deref(), a.edit), (Some("eins drei"), at(4)));
        assert_eq!(apply("abc", at(0), Command::DeleteWordBack).value, None);
        let sel = Edit { caret: 1, anchor: Some(3) };
        assert_eq!(apply("abcd", sel, Command::DeleteWordBack).value.as_deref(), Some("ad"));
        assert!(Command::DeleteWordBack.deletes() && !Command::WordLeft.deletes());
    }

    #[test]
    fn the_pointer_places_extends_and_multi_clicks_select() {
        let v = "Hallo Welt";
        assert_eq!(place(v, at(10), 3, false), at(3), "a click places");
        let e = place(v, at(2), 8, true);
        assert_eq!(selected_text(v, e).as_deref(), Some("llo We"), "Shift+click extends");
        let e = place(v, e, 9, true);
        assert_eq!(e.anchor, Some(2), "a drag keeps the selection's start");
        assert_eq!(selected_text(v, select_at(v, 7, 2)).as_deref(), Some("Welt"));
        assert_eq!(selected_text(v, select_at(v, 7, 3)).as_deref(), Some("Hallo Welt"));
        assert_eq!(place(v, at(0), 99, false), at(10), "past the end lands at the end");
    }
}
