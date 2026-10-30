// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: word boundaries for the word commands (Ctrl/Ctrl+Shift+arrows, Ctrl+Backspace and
//! Ctrl+Delete — TASK-0346 P2) and the double click: a word is a run of alphanumeric chars or
//! `_`; any other char separates. Char indices, allocation-free (the value is walked in place).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below

use crate::byte_at;

/// Whether `c` belongs to a word.
#[must_use]
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The start of the word before `caret` — separators skipped first (Ctrl+Left).
#[must_use]
pub fn word_left(value: &str, caret: usize) -> usize {
    let mut at = caret;
    let mut back = value[..byte_at(value, caret)].chars().rev().peekable();
    while back.next_if(|c| !is_word(*c)).is_some() {
        at -= 1;
    }
    while back.next_if(|c| is_word(*c)).is_some() {
        at -= 1;
    }
    at
}

/// The end of the word after `caret` — separators skipped first (Ctrl+Right).
#[must_use]
pub fn word_right(value: &str, caret: usize) -> usize {
    let mut at = caret;
    let mut ahead = value[byte_at(value, caret)..].chars().peekable();
    while ahead.next_if(|c| !is_word(*c)).is_some() {
        at += 1;
    }
    while ahead.next_if(|c| is_word(*c)).is_some() {
        at += 1;
    }
    at
}

/// The run of chars of one kind (a word, or separators) around `idx` — a double click's range.
/// At the end of the value the char before it decides.
#[must_use]
pub fn run_at(value: &str, idx: usize) -> (usize, usize) {
    let len = value.chars().count();
    if len == 0 {
        return (0, 0);
    }
    let pivot = idx.min(len - 1);
    let Some(kind) = value.chars().nth(pivot).map(is_word) else { return (idx, idx) };
    let start = pivot
        - value[..byte_at(value, pivot)].chars().rev().take_while(|c| is_word(*c) == kind).count();
    let end =
        pivot + value[byte_at(value, pivot)..].chars().take_while(|c| is_word(*c) == kind).count();
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_arrows_jump_over_words_and_their_separators() {
        let v = "Hallo, liebe Welt";
        assert_eq!(word_left(v, 17), 13, "to the start of `Welt`");
        assert_eq!(word_left(v, 13), 7, "over the space to `liebe`");
        assert_eq!(word_left(v, 7), 0, "over `, ` to `Hallo`");
        assert_eq!(word_left(v, 0), 0);
        assert_eq!(word_right(v, 0), 5, "to the end of `Hallo`");
        assert_eq!(word_right(v, 5), 12, "over `, ` to the end of `liebe`");
        assert_eq!(word_right(v, 17), 17);
    }

    #[test]
    fn words_count_chars_not_bytes() {
        let v = "Grüße über";
        assert_eq!(word_right(v, 0), 5);
        assert_eq!(word_left(v, 10), 6);
    }

    #[test]
    fn a_double_click_takes_the_run_it_lands_in() {
        let v = "ab  cd_e!";
        assert_eq!(run_at(v, 0), (0, 2));
        assert_eq!(run_at(v, 1), (0, 2));
        assert_eq!(run_at(v, 2), (2, 4), "the spaces are a run of their own");
        assert_eq!(run_at(v, 5), (4, 8), "`_` belongs to the word");
        assert_eq!(run_at(v, 9), (8, 9), "the end: the char before it decides");
        assert_eq!(run_at("", 3), (0, 0));
    }
}
