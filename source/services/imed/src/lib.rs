// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: imed — the IME authority (RFC-0075): text-focus state + key
//! composition (ime-core) + commit/action push planning. This crate half is
//! host-testable and IPC-free; `os_lite` binds it to the wire.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable for RFC-0075 Phase 1
//! TEST_COVERAGE: Unit tests below drive the full focus/key state machine.
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub mod os_lite;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod statefs;

use ime_core::{
    CandidatePage, Engine, EngineId, EngineOutcome, ImeAction, ImeEngine, ImeKey, TextRun,
    CANDIDATE_PAGE_MAX,
};
use ime_ranker::{BlobIo, Bucket, PersistentStore, CAND_MAX};
use nexus_wire::imed as wire;

/// UART marker proving imed is registered and serving (RFC-0075 semantics:
/// emitted only after the serve loop is armed — never by a stub).
pub const READY_MARKER: &str = "imed: ready";

/// The focused field as relayed by windowd (`OP_SET_FOCUS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusState {
    pub surface_id: u64,
    pub field_kind: u8,
}

/// Committed text for one push (RFC-0075 bound = one `TextRun`, ≤ 64 B —
/// a CJK candidate commit like 日本語 fits; Latin steps use ≤ 8 B of it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitText {
    bytes: [u8; wire::TEXT_MAX_BYTES],
    len: u8,
}

impl Default for CommitText {
    fn default() -> Self {
        Self { bytes: [0; wire::TEXT_MAX_BYTES], len: 0 }
    }
}

impl CommitText {
    fn from_str(text: &str) -> Self {
        let mut out = Self::default();
        let bytes = text.as_bytes();
        let n = bytes.len().min(out.bytes.len());
        out.bytes[..n].copy_from_slice(&bytes[..n]);
        out.len = n as u8;
        out
    }

    fn from_char(ch: char) -> Self {
        let mut buf = [0u8; 4];
        Self::from_str(ch.encode_utf8(&mut buf))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// What one key produced for the focused surface (both may be set: a dead
/// key flushed by Enter commits the accent AND passes the action through).
/// The CJK engines add preedit/candidate snapshots (`None` = unchanged;
/// `Some(empty)` = clear).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyPushes {
    pub surface_id: u64,
    pub commit: Option<CommitText>,
    pub action: Option<u8>,
    /// Preedit snapshot to push (composition preview; empty clears).
    pub preedit: Option<TextRun>,
    /// Candidate page to push: (page, total, up-to-8 texts).
    pub candidates: Option<ime_core::CandidatePage>,
}

/// The IME state machine (RFC-0075 Phase 3 semantics): COMPOSITION is
/// focus-independent (the engine always runs — the deterministic osk probe
/// exercises it without a field), DELIVERY is focus-gated (pushes exist
/// only while a surface holds text focus), and any focus TRANSITION resets
/// composition (half-typed state never leaks across fields). Password
/// fields BYPASS the engine entirely: direct commit, no preedit, no
/// candidates, no learning — fail-closed at this layer.
#[derive(Debug)]
pub struct ImedCore {
    engine: Engine,
    /// The active layout tag (cycle guard for OSK-driven persistence).
    layout: [u8; 8],
    layout_len: u8,
    focus: Option<FocusState>,
    /// Non-empty preedit/candidates were pushed — an empty snapshot must
    /// follow once to CLEAR the strip (then stop pushing empties).
    strip_dirty: bool,
    /// TASK-0204: adaptive-ranking personalization store for the active locale
    /// (loaded by `os_lite` from statefsd on layout switch, flushed on focus
    /// loss). Ranks the candidate strip and learns from commits.
    store: PersistentStore,
    /// The previously committed candidate's bytes — the `(prev, cand)` bigram
    /// context. Reset on any focus/layout change (no cross-field learning).
    last_commit: [u8; CAND_MAX],
    last_commit_len: u8,
    /// Coarse recency bucket: bumped once per focus-gain (a new editing
    /// session), never a raw timestamp.
    bucket: Bucket,
}

impl Default for ImedCore {
    fn default() -> Self {
        Self::new()
    }
}

/// Outcome of one engine step, before delivery gating.
#[derive(Debug, Clone, Copy)]
pub struct StepEcho {
    /// The commit this step produced (probe echo; empty = none).
    pub commit: CommitText,
}

impl ImedCore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            engine: Engine::new(EngineId::Latin),
            layout: [0; 8],
            layout_len: 0,
            focus: None,
            strip_dirty: false,
            // Personalization defaults ON; the `ime.personalization` Settings
            // toggle is wired in a later cut (2b-Settings).
            store: PersistentStore::new(true),
            last_commit: [0; CAND_MAX],
            last_commit_len: 0,
            bucket: 0,
        }
    }

    /// The last applied layout tag (empty until the first switch).
    #[must_use]
    pub fn layout_tag(&self) -> &str {
        core::str::from_utf8(&self.layout[..usize::from(self.layout_len)]).unwrap_or("")
    }

    #[must_use]
    pub fn focus(&self) -> Option<FocusState> {
        self.focus
    }

    /// Switches the composition engine (`input.keymap` relay / OSK globe).
    /// A switch resets composition state.
    pub fn set_layout(&mut self, layout: &str) {
        self.engine = Engine::new(EngineId::for_layout(layout));
        self.strip_dirty = false;
        self.reset_last_commit(); // no bigram across a language switch
        let b = layout.as_bytes();
        let n = b.len().min(self.layout.len());
        self.layout[..n].copy_from_slice(&b[..n]);
        self.layout_len = n as u8;
    }

    /// Applies a windowd focus relay. Any focus TRANSITION cancels pending
    /// composition state (a half-typed accent never leaks across fields).
    pub fn set_focus(&mut self, surface_id: u64, focused: bool, field_kind: u8) {
        let next = if focused { Some(FocusState { surface_id, field_kind }) } else { None };
        if next != self.focus {
            self.engine.reset();
            self.strip_dirty = false;
            // No `(prev, cand)` bigram across a field boundary.
            self.reset_last_commit();
            // A new editing session advances the coarse recency bucket.
            if focused {
                self.bucket = self.bucket.saturating_add(1);
            }
        }
        self.focus = next;
    }

    fn password_focused(&self) -> bool {
        self.focus.is_some_and(|f| f.field_kind == wire::FIELD_KIND_PASSWORD)
    }

    fn last_commit_bytes(&self) -> Option<&[u8]> {
        (self.last_commit_len > 0).then(|| &self.last_commit[..usize::from(self.last_commit_len)])
    }

    fn set_last_commit(&mut self, bytes: &[u8]) {
        let n = bytes.len().min(CAND_MAX);
        self.last_commit[..n].copy_from_slice(&bytes[..n]);
        self.last_commit_len = n as u8;
    }

    fn reset_last_commit(&mut self) {
        self.last_commit_len = 0;
    }

    /// statefsd key for the active locale's personalization blob.
    fn store_key(&self) -> alloc::string::String {
        let lang = if self.layout_len == 0 { "us" } else { self.layout_tag() };
        alloc::format!("/state/ime/{lang}/personal")
    }

    /// Reranks a candidate page by the personalization store. Identity when the
    /// page has 0/1 items (the store's own gate handles disabled/password).
    fn rank_candidates(&self, page: &CandidatePage) -> CandidatePage {
        let n = page.len();
        if n <= 1 {
            return *page;
        }
        let mut cands: [&[u8]; CANDIDATE_PAGE_MAX] = [b"".as_slice(); CANDIDATE_PAGE_MAX];
        for (i, slot) in cands.iter_mut().enumerate().take(n) {
            if let Some(c) = page.get(i) {
                *slot = c.as_str().as_bytes();
            }
        }
        let order = self.store.rank(self.last_commit_bytes(), &cands[..n], self.bucket);
        page.reordered(&order)
    }

    /// Applies the `ime.personalization` toggle (settingsd SSOT). OFF drops all
    /// in-memory learning immediately and stops training/ranking/IO — the
    /// privacy invariant (RFC-0075: off = no reads, no writes, no learning).
    pub fn set_personalization(&mut self, on: bool) {
        self.store.set_enabled(on);
    }

    /// Whether personalization is currently on.
    #[must_use]
    pub fn personalization_enabled(&self) -> bool {
        self.store.is_enabled()
    }

    /// The "forget learned words" action: clears the store (and marks it dirty
    /// so the next flush truncates the on-disk blob).
    pub fn forget_learned(&mut self) {
        self.store.forget_all();
    }

    /// Loads the active locale's personalization blob (statefsd on OS, a fake in
    /// host tests). Called on engine activation / layout switch.
    pub fn load_store<B: BlobIo>(&mut self, io: &B) {
        let key = self.store_key();
        self.store.load(io, &key);
    }

    /// Flushes the personalization blob back if dirty; returns whether it wrote.
    /// Called on focus loss.
    pub fn flush_store<B: BlobIo>(&mut self, io: &mut B) -> bool {
        let key = self.store_key();
        self.store.flush(io, &key)
    }

    /// Test-only: how many candidates the store has learned (0 = nothing).
    #[cfg(test)]
    fn learned_count(&self) -> usize {
        use ime_ranker::PersonalStore;
        self.store.store().dict_entries().len()
    }

    /// Converts an engine outcome into a focused-surface push plan.
    fn plan(&mut self, outcome: &EngineOutcome) -> (Option<KeyPushes>, StepEcho) {
        let echo = StepEcho { commit: CommitText::from_str(outcome.commit.as_str()) };
        let Some(focus) = self.focus else {
            return (None, echo); // composition ran; delivery is focus-gated
        };
        // TASK-0204: learn from a commit — NEVER for password fields (security
        // invariant). Bumps the committed candidate's frequency, the
        // `(prev, cand)` bigram, and its recency bucket; the store's own gate
        // no-ops when personalization is off.
        if !outcome.commit.is_empty() && !self.password_focused() {
            // Copy `prev` to a stack buffer so the store can be borrowed mutably.
            let prev_len = usize::from(self.last_commit_len);
            let mut prev_buf = [0u8; CAND_MAX];
            prev_buf[..prev_len].copy_from_slice(&self.last_commit[..prev_len]);
            let prev = (prev_len > 0).then_some(&prev_buf[..prev_len]);
            self.store.train(prev, outcome.commit.as_str().as_bytes(), self.bucket);
            self.set_last_commit(outcome.commit.as_str().as_bytes());
        }
        let commit =
            (!outcome.commit.is_empty()).then(|| CommitText::from_str(outcome.commit.as_str()));
        let action = outcome.pass_action.map(encode_action);
        // Strip snapshots: push while non-empty; after a non-empty run push
        // ONE empty snapshot to clear (never a steady stream of empties).
        // Password fields never see a strip (security invariant).
        let strip_active = !outcome.preedit.is_empty() || !outcome.candidates.is_empty();
        let (preedit, candidates) = if self.password_focused() {
            (None, None)
        } else if strip_active {
            self.strip_dirty = true;
            // Personalization reorders the visible candidates (best-learned
            // first); untrained candidates keep the engine's table order.
            (Some(outcome.preedit), Some(self.rank_candidates(&outcome.candidates)))
        } else if self.strip_dirty {
            self.strip_dirty = false;
            (Some(TextRun::empty()), Some(ime_core::CandidatePage::empty()))
        } else {
            (None, None)
        };
        if commit.is_none() && action.is_none() && preedit.is_none() && candidates.is_none() {
            return (None, echo);
        }
        (
            Some(KeyPushes { surface_id: focus.surface_id, commit, action, preedit, candidates }),
            echo,
        )
    }

    /// Feeds one resolved key (wire `KEY_KIND_*`/`ACTION_*` vocabulary).
    /// Returns the focused-surface push plan (None = nothing to deliver)
    /// plus the probe echo.
    pub fn key(&mut self, kind: u8, ch: u32, action: u8) -> (Option<KeyPushes>, StepEcho) {
        let empty_echo = StepEcho { commit: CommitText::default() };
        let Some(key) = decode_key(kind, ch, action) else {
            return (None, empty_echo);
        };
        // Password bypass: no composition, no preview, no learning — text
        // commits directly, actions pass through.
        if self.password_focused() {
            let Some(focus) = self.focus else {
                return (None, empty_echo); // unreachable: password implies focus
            };
            return match key {
                ImeKey::Text(ch) => {
                    let commit = CommitText::from_char(ch);
                    (
                        Some(KeyPushes {
                            surface_id: focus.surface_id,
                            commit: Some(commit),
                            ..KeyPushes::default()
                        }),
                        StepEcho { commit },
                    )
                }
                ImeKey::Action(act) => (
                    Some(KeyPushes {
                        surface_id: focus.surface_id,
                        action: Some(encode_action(act)),
                        ..KeyPushes::default()
                    }),
                    empty_echo,
                ),
                ImeKey::Dead(_) => (None, empty_echo),
            };
        }
        let outcome = self.engine.feed(key);
        if outcome.handled {
            return self.plan(&outcome);
        }
        // Engine passed the key through: text commits directly, actions
        // pass through unchanged.
        let echo_commit = match key {
            ImeKey::Text(ch) => CommitText::from_char(ch),
            _ => CommitText::default(),
        };
        let echo = StepEcho { commit: echo_commit };
        let Some(focus) = self.focus else {
            return (None, echo);
        };
        let pushes = match key {
            ImeKey::Text(_) => Some(KeyPushes {
                surface_id: focus.surface_id,
                commit: Some(echo_commit),
                ..KeyPushes::default()
            }),
            ImeKey::Action(act) => Some(KeyPushes {
                surface_id: focus.surface_id,
                action: Some(encode_action(act)),
                ..KeyPushes::default()
            }),
            ImeKey::Dead(_) => None,
        };
        (pushes, echo)
    }

    /// Commits candidate `index` of the current page (windowd relay or the
    /// vetted OSK route).
    pub fn candidate_select(&mut self, index: usize) -> (Option<KeyPushes>, StepEcho) {
        let outcome = self.engine.select(index);
        if !outcome.handled {
            return (None, StepEcho { commit: CommitText::default() });
        }
        self.plan(&outcome)
    }
}

fn decode_key(kind: u8, ch: u32, action: u8) -> Option<ImeKey> {
    match kind {
        wire::KEY_KIND_TEXT => Some(ImeKey::Text(char::from_u32(ch)?)),
        wire::KEY_KIND_DEAD => Some(ImeKey::Dead(char::from_u32(ch)?)),
        wire::KEY_KIND_ACTION => Some(ImeKey::Action(match action {
            wire::ACTION_ENTER => ImeAction::Enter,
            wire::ACTION_ESCAPE => ImeAction::Escape,
            wire::ACTION_BACKSPACE => ImeAction::Backspace,
            wire::ACTION_TAB => ImeAction::Tab,
            _ => return None,
        })),
        _ => None,
    }
}

fn encode_action(action: ImeAction) -> u8 {
    match action {
        ImeAction::Enter => wire::ACTION_ENTER,
        ImeAction::Escape => wire::ACTION_ESCAPE,
        ImeAction::Backspace => wire::ACTION_BACKSPACE,
        ImeAction::Tab => wire::ACTION_TAB,
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
