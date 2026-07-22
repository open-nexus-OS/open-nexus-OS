// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0075 / TASK-0149 — the `ImeEngine` trait: ONE deterministic
//! composition contract every language engine implements (Latin dead keys,
//! JP romaji→kana→kanji, KR 2-set jamo, ZH pinyin). imed hosts any engine
//! behind [`Engine`] (enum dispatch, no_std/alloc-free) and never needs
//! language-specific code. All outputs are bounded value snapshots.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (RFC-0075 Phase 3 seam)
//! TEST_COVERAGE: `tests/cjk_contract.rs` (goldens + engine-swap + no-panic).
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use crate::compose::Composer;
use crate::jp::JpEngine;
use crate::kr::KrEngine;
use crate::outcome::{ImeAction, ImeKey, Preedit};
use crate::zh::ZhEngine;

/// Bounded committed/preedit text run (same 64-byte bound as the preedit).
pub type TextRun = Preedit;

/// Candidates per page (RFC-0075 bound: ≤ 8 × 32 B).
pub const CANDIDATE_PAGE_MAX: usize = 8;
/// Maximum bytes per candidate string.
pub const CANDIDATE_MAX_BYTES: usize = 32;

/// One bounded candidate string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    bytes: [u8; CANDIDATE_MAX_BYTES],
    len: u8,
}

impl Default for Candidate {
    fn default() -> Self {
        Self::empty()
    }
}

impl Candidate {
    #[must_use]
    pub const fn empty() -> Self {
        Self { bytes: [0; CANDIDATE_MAX_BYTES], len: 0 }
    }

    /// Builds a candidate; oversized input is rejected (`None`, fail-closed).
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.is_empty() || bytes.len() > CANDIDATE_MAX_BYTES {
            return None;
        }
        let mut c = Self::empty();
        c.bytes[..bytes.len()].copy_from_slice(bytes);
        c.len = bytes.len() as u8;
        Some(c)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

/// One PAGE of candidates (a bounded window over the engine's match list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CandidatePage {
    items: [Candidate; CANDIDATE_PAGE_MAX],
    len: u8,
    /// Zero-based page index this window shows.
    pub page: u8,
    /// Total matches across all pages (for "1/3"-style UI).
    pub total: u8,
}

impl CandidatePage {
    #[must_use]
    pub const fn empty() -> Self {
        Self { items: [Candidate::empty(); CANDIDATE_PAGE_MAX], len: 0, page: 0, total: 0 }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    /// Item `i` of THIS page, if present.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<&Candidate> {
        if i < usize::from(self.len) {
            self.items.get(i)
        } else {
            None
        }
    }

    /// Appends an item; silently full beyond the page bound (callers window
    /// their lists — the page is a VIEW, not a growable container).
    pub(crate) fn push(&mut self, c: Candidate) {
        if usize::from(self.len) < CANDIDATE_PAGE_MAX {
            self.items[usize::from(self.len)] = c;
            self.len += 1;
        }
    }
}

/// Result of one engine step — a full bounded SNAPSHOT (preedit + candidate
/// page reflect the state AFTER the step; deterministic for a given key
/// sequence, always).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineOutcome {
    /// True when the engine consumed the key (nothing passes downstream).
    pub handled: bool,
    /// Text committed by this step (empty = none).
    pub commit: TextRun,
    /// Composition preview after this step (empty = no active composition).
    pub preedit: TextRun,
    /// Candidate page after this step (empty = no candidates showing).
    pub candidates: CandidatePage,
    /// Action to pass downstream after a flush (e.g. Enter after commit).
    pub pass_action: Option<ImeAction>,
}

impl EngineOutcome {
    /// An unhandled step (key passes through untouched).
    #[must_use]
    pub fn pass() -> Self {
        Self::default()
    }
}

/// The ONE composition contract (RFC-0075): deterministic, total (every key
/// has a defined outcome in every state), bounded outputs only.
pub trait ImeEngine {
    /// Feeds one resolved key.
    fn feed(&mut self, key: ImeKey) -> EngineOutcome;
    /// Commits candidate `index` of the CURRENT page, if present.
    fn select(&mut self, index: usize) -> EngineOutcome;
    /// Advances to the next candidate page (wraps to the first).
    fn page_next(&mut self) -> EngineOutcome;
    /// Drops all composition state (focus loss / surface switch).
    fn reset(&mut self);
}

/// Engine selection by layout family (`input.keymap`: us/de → Latin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineId {
    Latin,
    Jp,
    Kr,
    Zh,
}

impl EngineId {
    /// Maps a keymap layout name onto its engine (unknown = Latin,
    /// fail-open to plain typing — never a dead keyboard).
    #[must_use]
    pub fn for_layout(layout: &str) -> Self {
        match layout {
            "jp" => Self::Jp,
            "kr" => Self::Kr,
            "zh" => Self::Zh,
            _ => Self::Latin,
        }
    }
}

/// Enum-dispatch host for the shipped engines (no_std, no alloc, no dyn).
#[derive(Debug, Clone, Copy)]
pub enum Engine {
    Latin(Composer),
    Jp(JpEngine),
    Kr(KrEngine),
    Zh(ZhEngine),
}

impl Engine {
    #[must_use]
    pub fn new(id: EngineId) -> Self {
        match id {
            EngineId::Latin => Self::Latin(Composer::new()),
            EngineId::Jp => Self::Jp(JpEngine::new()),
            EngineId::Kr => Self::Kr(KrEngine::new()),
            EngineId::Zh => Self::Zh(ZhEngine::new()),
        }
    }
}

impl ImeEngine for Engine {
    fn feed(&mut self, key: ImeKey) -> EngineOutcome {
        match self {
            // The Latin composer keeps its Phase-0 outcome shape; adapt it
            // into the engine snapshot (no preedit, no candidates).
            Self::Latin(c) => {
                let out = c.feed(key);
                let mut commit = TextRun::empty();
                for ch in out.commit.chars() {
                    let _ = commit.push(ch);
                }
                EngineOutcome {
                    handled: out.handled,
                    commit,
                    pass_action: out.pass_action,
                    ..EngineOutcome::default()
                }
            }
            Self::Jp(e) => e.feed(key),
            Self::Kr(e) => e.feed(key),
            Self::Zh(e) => e.feed(key),
        }
    }

    fn select(&mut self, index: usize) -> EngineOutcome {
        match self {
            Self::Latin(_) => EngineOutcome::pass(),
            Self::Jp(e) => e.select(index),
            Self::Kr(e) => e.select(index),
            Self::Zh(e) => e.select(index),
        }
    }

    fn page_next(&mut self) -> EngineOutcome {
        match self {
            Self::Latin(_) => EngineOutcome::pass(),
            Self::Jp(e) => e.page_next(),
            Self::Kr(e) => e.page_next(),
            Self::Zh(e) => e.page_next(),
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Latin(c) => c.reset(),
            Self::Jp(e) => e.reset(),
            Self::Kr(e) => e.reset(),
            Self::Zh(e) => e.reset(),
        }
    }
}

/// Windows a full match list into page `page` (shared by the engines).
pub(crate) fn page_of(matches: &[&str], page: u8) -> CandidatePage {
    let total = matches.len().min(u8::MAX as usize) as u8;
    if total == 0 {
        return CandidatePage::empty();
    }
    let pages = total.div_ceil(CANDIDATE_PAGE_MAX as u8);
    let page = page % pages.max(1);
    let start = usize::from(page) * CANDIDATE_PAGE_MAX;
    let mut out = CandidatePage { page, total, ..CandidatePage::empty() };
    for text in matches.iter().skip(start).take(CANDIDATE_PAGE_MAX) {
        if let Some(c) = Candidate::new(text) {
            out.push(c);
        }
    }
    out
}
