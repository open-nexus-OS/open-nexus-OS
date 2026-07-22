// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0149 — Korean engine: 2-set (dubeolsik) hangul composition.
//! Latin keys map to jamo (jamo input is accepted directly too); syllables
//! compose via the standard Unicode algebra `0xAC00 + (cho·21 + jung)·28 +
//! jong` with compound-vowel/-final tables and jamo-splitting backspace.
//! Deterministic and bounded; no lexicon (hangul commits as typed).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (RFC-0075 Phase 3 seam)
//! TEST_COVERAGE: `tests/cjk_contract.rs` (한 golden, backspace split,
//! jong steal, compounds, no-panic stream).
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use crate::engine::{EngineOutcome, ImeEngine, TextRun};
use crate::outcome::{ImeAction, ImeKey};

/// 2-set layout: Latin key → jamo (shifted variants ride uppercase).
const LATIN_JAMO: &[(char, char)] = &[
    ('q', 'ㅂ'),
    ('w', 'ㅈ'),
    ('e', 'ㄷ'),
    ('r', 'ㄱ'),
    ('t', 'ㅅ'),
    ('y', 'ㅛ'),
    ('u', 'ㅕ'),
    ('i', 'ㅑ'),
    ('o', 'ㅐ'),
    ('p', 'ㅔ'),
    ('a', 'ㅁ'),
    ('s', 'ㄴ'),
    ('d', 'ㅇ'),
    ('f', 'ㄹ'),
    ('g', 'ㅎ'),
    ('h', 'ㅗ'),
    ('j', 'ㅓ'),
    ('k', 'ㅏ'),
    ('l', 'ㅣ'),
    ('z', 'ㅋ'),
    ('x', 'ㅌ'),
    ('c', 'ㅊ'),
    ('v', 'ㅍ'),
    ('b', 'ㅠ'),
    ('n', 'ㅜ'),
    ('m', 'ㅡ'),
    ('Q', 'ㅃ'),
    ('W', 'ㅉ'),
    ('E', 'ㄸ'),
    ('R', 'ㄲ'),
    ('T', 'ㅆ'),
    ('O', 'ㅒ'),
    ('P', 'ㅖ'),
];

/// Initials (cho) in Unicode order.
const CHO: &[char] = &[
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ',
    'ㅌ', 'ㅍ', 'ㅎ',
];
/// Medials (jung) in Unicode order.
const JUNG: &[char] = &[
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ',
    'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
];
/// Finals (jong) in Unicode order; index 0 = none.
const JONG: &[char] = &[
    '\0', 'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ',
    'ㅁ', 'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

/// Compound medials: (first, second) → compound.
const JUNG_COMPOUND: &[(char, char, char)] = &[
    ('ㅗ', 'ㅏ', 'ㅘ'),
    ('ㅗ', 'ㅐ', 'ㅙ'),
    ('ㅗ', 'ㅣ', 'ㅚ'),
    ('ㅜ', 'ㅓ', 'ㅝ'),
    ('ㅜ', 'ㅔ', 'ㅞ'),
    ('ㅜ', 'ㅣ', 'ㅟ'),
    ('ㅡ', 'ㅣ', 'ㅢ'),
];

/// Compound finals: (first, second) → compound.
const JONG_COMPOUND: &[(char, char, char)] = &[
    ('ㄱ', 'ㅅ', 'ㄳ'),
    ('ㄴ', 'ㅈ', 'ㄵ'),
    ('ㄴ', 'ㅎ', 'ㄶ'),
    ('ㄹ', 'ㄱ', 'ㄺ'),
    ('ㄹ', 'ㅁ', 'ㄻ'),
    ('ㄹ', 'ㅂ', 'ㄼ'),
    ('ㄹ', 'ㅅ', 'ㄽ'),
    ('ㄹ', 'ㅌ', 'ㄾ'),
    ('ㄹ', 'ㅍ', 'ㄿ'),
    ('ㄹ', 'ㅎ', 'ㅀ'),
];

fn jamo_of(ch: char) -> char {
    LATIN_JAMO.iter().find(|(l, _)| *l == ch).map_or(ch, |(_, j)| *j)
}

fn cho_index(j: char) -> Option<usize> {
    CHO.iter().position(|&c| c == j)
}
fn jung_index(j: char) -> Option<usize> {
    JUNG.iter().position(|&c| c == j)
}
fn jong_index(j: char) -> Option<usize> {
    JONG.iter().position(|&c| c == j).filter(|&i| i > 0)
}

fn jung_compound(a: char, b: char) -> Option<char> {
    JUNG_COMPOUND.iter().find(|(x, y, _)| *x == a && *y == b).map(|(_, _, c)| *c)
}
fn jong_compound(a: char, b: char) -> Option<char> {
    JONG_COMPOUND.iter().find(|(x, y, _)| *x == a && *y == b).map(|(_, _, c)| *c)
}
fn jong_split(j: char) -> Option<(char, char)> {
    JONG_COMPOUND.iter().find(|(_, _, c)| *c == j).map(|(a, b, _)| (*a, *b))
}
fn jung_split(j: char) -> Option<(char, char)> {
    JUNG_COMPOUND.iter().find(|(_, _, c)| *c == j).map(|(a, b, _)| (*a, *b))
}

/// The syllable under composition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Syllable {
    cho: Option<char>,
    jung: Option<char>,
    jong: Option<char>,
}

impl Syllable {
    fn is_empty(&self) -> bool {
        self.cho.is_none() && self.jung.is_none() && self.jong.is_none()
    }

    /// Renders the current state (block when composable, bare jamo else).
    fn render(&self) -> Option<char> {
        match (self.cho, self.jung, self.jong) {
            (Some(c), Some(v), jong) => {
                let (ci, vi) = (cho_index(c)?, jung_index(v)?);
                let ji = jong.and_then(jong_index).unwrap_or(0);
                char::from_u32(0xAC00 + ((ci as u32 * 21 + vi as u32) * 28) + ji as u32)
            }
            (Some(c), None, _) => Some(c),
            (None, Some(v), _) => Some(v),
            _ => None,
        }
    }
}

/// Deterministic 2-set hangul engine.
#[derive(Debug, Clone, Copy)]
pub struct KrEngine {
    /// Completed syllables of the active composition run.
    text: TextRun,
    cur: Syllable,
}

impl Default for KrEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl KrEngine {
    #[must_use]
    pub const fn new() -> Self {
        Self { text: TextRun::empty(), cur: Syllable::default_const() }
    }

    fn flush_cur(&mut self) {
        if let Some(ch) = self.cur.render() {
            let _ = self.text.push(ch);
        }
        self.cur = Syllable::default();
    }

    fn snapshot(&self, handled: bool, commit: TextRun) -> EngineOutcome {
        let mut preedit = self.text;
        if let Some(ch) = self.cur.render() {
            let _ = preedit.push(ch);
        }
        EngineOutcome { handled, commit, preedit, ..EngineOutcome::default() }
    }

    fn feed_jamo(&mut self, j: char) {
        let is_vowel = jung_index(j).is_some();
        if is_vowel {
            match (self.cur.jung, self.cur.jong) {
                // Jong steal: the final consonant becomes the next initial.
                (_, Some(jong)) => {
                    let (keep, steal) =
                        jong_split(jong).map_or((None, jong), |(a, b)| (Some(a), b));
                    self.cur.jong = keep;
                    self.flush_cur();
                    self.cur = Syllable { cho: Some(steal), jung: Some(j), jong: None };
                }
                (Some(v), None) => {
                    if let Some(comp) = jung_compound(v, j) {
                        self.cur.jung = Some(comp);
                    } else {
                        self.flush_cur();
                        self.cur.jung = Some(j);
                    }
                }
                (None, None) => self.cur.jung = Some(j),
            }
            return;
        }
        // Consonant.
        match (self.cur.cho, self.cur.jung, self.cur.jong) {
            (None, _, _) => self.cur.cho = Some(j),
            (Some(_), None, _) => {
                // Two bare initials cannot merge (ㄲ etc. arrive shifted):
                // finish the first, start anew.
                self.flush_cur();
                self.cur.cho = Some(j);
            }
            (Some(_), Some(_), None) => {
                if jong_index(j).is_some() {
                    self.cur.jong = Some(j);
                } else {
                    self.flush_cur();
                    self.cur.cho = Some(j);
                }
            }
            (Some(_), Some(_), Some(jong)) => {
                if let Some(comp) = jong_compound(jong, j) {
                    self.cur.jong = Some(comp);
                } else {
                    self.flush_cur();
                    self.cur.cho = Some(j);
                }
            }
        }
    }

    fn commit_all(&mut self) -> TextRun {
        self.flush_cur();
        let out = self.text;
        self.reset();
        out
    }
}

impl Syllable {
    const fn default_const() -> Self {
        Self { cho: None, jung: None, jong: None }
    }
}

impl ImeEngine for KrEngine {
    fn feed(&mut self, key: ImeKey) -> EngineOutcome {
        match key {
            ImeKey::Text(ch) => {
                let j = jamo_of(ch);
                let is_jamo = cho_index(j).is_some() || jung_index(j).is_some();
                if !is_jamo {
                    // Non-jamo input (space, digits …) commits + passes.
                    if self.text.is_empty() && self.cur.is_empty() {
                        return EngineOutcome::pass();
                    }
                    let commit = self.commit_all();
                    return EngineOutcome { commit, ..EngineOutcome::pass() };
                }
                self.feed_jamo(j);
                self.snapshot(true, TextRun::empty())
            }
            ImeKey::Dead(_) => EngineOutcome::pass(),
            ImeKey::Action(ImeAction::Backspace) => {
                // Jamo split: jong → (cho,jung); compound reduces first.
                if let Some(jong) = self.cur.jong {
                    self.cur.jong = jong_split(jong).map(|(a, _)| a);
                    return self.snapshot(true, TextRun::empty());
                }
                if let Some(jung) = self.cur.jung {
                    self.cur.jung = jung_split(jung).map(|(a, _)| a);
                    return self.snapshot(true, TextRun::empty());
                }
                if self.cur.cho.take().is_some() || self.text.pop() {
                    return self.snapshot(true, TextRun::empty());
                }
                EngineOutcome::pass()
            }
            ImeKey::Action(ImeAction::Enter) => {
                if self.text.is_empty() && self.cur.is_empty() {
                    return EngineOutcome::pass();
                }
                let commit = self.commit_all();
                self.snapshot(true, commit)
            }
            ImeKey::Action(ImeAction::Escape) => {
                if self.text.is_empty() && self.cur.is_empty() {
                    return EngineOutcome::pass();
                }
                self.reset();
                self.snapshot(true, TextRun::empty())
            }
            ImeKey::Action(a) => {
                if self.text.is_empty() && self.cur.is_empty() {
                    return EngineOutcome::pass();
                }
                let commit = self.commit_all();
                EngineOutcome { handled: true, commit, pass_action: Some(a), ..Default::default() }
            }
        }
    }

    fn select(&mut self, _index: usize) -> EngineOutcome {
        EngineOutcome::pass() // no candidate model (hangul commits as typed)
    }

    fn page_next(&mut self) -> EngineOutcome {
        EngineOutcome::pass()
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}
