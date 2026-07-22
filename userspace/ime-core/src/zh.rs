// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0149 — Chinese engine: pinyin buffer → han candidates from
//! a bounded const table (simplified; exact-buffer lookup — general
//! segmentation is a later slice). Deterministic order = table order; the
//! `shi` row exceeds one page on purpose (paging proof). Bounded everywhere.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (RFC-0075 Phase 3 seam)
//! TEST_COVERAGE: `tests/cjk_contract.rs` (nihao golden, paging, no-panic).
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use crate::engine::{page_of, CandidatePage, EngineOutcome, ImeEngine, TextRun};
use crate::outcome::{ImeAction, ImeKey};

/// Longest pinyin buffer the engine holds (multi-syllable phrases).
const PINYIN_MAX: usize = 12;

/// pinyin → han candidates (deterministic order = row order).
const PINYIN_HAN: &[(&str, &[&str])] = &[
    ("nihao", &["你好"]),
    ("ni", &["你", "泥", "尼"]),
    ("hao", &["好", "号", "毫"]),
    ("shi", &["是", "时", "十", "事", "世", "市", "师", "诗", "施", "石"]),
    ("wo", &["我"]),
    ("zhongwen", &["中文"]),
    ("zhong", &["中", "重", "种"]),
    ("xie", &["谢", "写", "鞋"]),
    ("xiexie", &["谢谢"]),
];

/// Deterministic pinyin engine (exact-buffer lookup).
#[derive(Debug, Clone, Copy)]
pub struct ZhEngine {
    pinyin: [u8; PINYIN_MAX],
    len: u8,
    page: u8,
    selecting: bool,
}

impl Default for ZhEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ZhEngine {
    #[must_use]
    pub const fn new() -> Self {
        Self { pinyin: [0; PINYIN_MAX], len: 0, page: 0, selecting: false }
    }

    fn pinyin_str(&self) -> &str {
        core::str::from_utf8(&self.pinyin[..usize::from(self.len)]).unwrap_or("")
    }

    fn matches(&self) -> &'static [&'static str] {
        PINYIN_HAN.iter().find(|(p, _)| *p == self.pinyin_str()).map(|(_, han)| *han).unwrap_or(&[])
    }

    fn snapshot(&self, handled: bool, commit: TextRun) -> EngineOutcome {
        let mut preedit = TextRun::empty();
        for ch in self.pinyin_str().chars() {
            let _ = preedit.push(ch);
        }
        let candidates = if self.selecting {
            page_of(self.matches(), self.page)
        } else {
            CandidatePage::empty()
        };
        EngineOutcome { handled, commit, preedit, candidates, ..EngineOutcome::default() }
    }

    /// Commits the raw pinyin buffer (no conversion — honest fallback).
    fn commit_raw(&mut self) -> TextRun {
        let mut out = TextRun::empty();
        for ch in self.pinyin_str().chars() {
            let _ = out.push(ch);
        }
        self.reset();
        out
    }
}

impl ImeEngine for ZhEngine {
    fn feed(&mut self, key: ImeKey) -> EngineOutcome {
        match key {
            ImeKey::Text(' ') => {
                if self.len == 0 {
                    return EngineOutcome::pass();
                }
                if self.selecting {
                    self.page = self.page.wrapping_add(1);
                } else {
                    self.selecting = true;
                    self.page = 0;
                }
                self.snapshot(true, TextRun::empty())
            }
            ImeKey::Text(ch) if ch.is_ascii_lowercase() => {
                self.selecting = false;
                if usize::from(self.len) < PINYIN_MAX {
                    self.pinyin[usize::from(self.len)] = ch as u8;
                    self.len += 1;
                }
                self.snapshot(true, TextRun::empty())
            }
            ImeKey::Text(_) | ImeKey::Dead(_) => {
                if self.len == 0 {
                    return EngineOutcome::pass();
                }
                let commit = self.commit_raw();
                EngineOutcome { commit, ..EngineOutcome::pass() }
            }
            ImeKey::Action(ImeAction::Backspace) => {
                if self.len == 0 {
                    return EngineOutcome::pass();
                }
                self.len -= 1;
                self.selecting = false;
                self.snapshot(true, TextRun::empty())
            }
            ImeKey::Action(ImeAction::Enter) => {
                if self.len == 0 {
                    return EngineOutcome::pass();
                }
                let commit = self.commit_raw();
                self.snapshot(true, commit)
            }
            ImeKey::Action(ImeAction::Escape) => {
                if self.len == 0 {
                    return EngineOutcome::pass();
                }
                self.reset();
                self.snapshot(true, TextRun::empty())
            }
            ImeKey::Action(a) => {
                if self.len == 0 {
                    return EngineOutcome::pass();
                }
                let commit = self.commit_raw();
                EngineOutcome { handled: true, commit, pass_action: Some(a), ..Default::default() }
            }
        }
    }

    fn select(&mut self, index: usize) -> EngineOutcome {
        if !self.selecting {
            return EngineOutcome::pass();
        }
        let snapshot = self.snapshot(true, TextRun::empty());
        let Some(candidate) = snapshot.candidates.get(index) else {
            return EngineOutcome::pass();
        };
        let mut commit = TextRun::empty();
        for ch in candidate.as_str().chars() {
            let _ = commit.push(ch);
        }
        self.reset();
        self.snapshot(true, commit)
    }

    fn page_next(&mut self) -> EngineOutcome {
        if !self.selecting {
            return EngineOutcome::pass();
        }
        self.page = self.page.wrapping_add(1);
        self.snapshot(true, TextRun::empty())
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}
