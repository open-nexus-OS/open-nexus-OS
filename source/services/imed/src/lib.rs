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

use ime_core::{Engine, EngineId, EngineOutcome, ImeAction, ImeEngine, ImeKey, TextRun};
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
        }
        self.focus = next;
    }

    fn password_focused(&self) -> bool {
        self.focus.is_some_and(|f| f.field_kind == wire::FIELD_KIND_PASSWORD)
    }

    /// Converts an engine outcome into a focused-surface push plan.
    fn plan(&mut self, outcome: &EngineOutcome) -> (Option<KeyPushes>, StepEcho) {
        let echo = StepEcho { commit: CommitText::from_str(outcome.commit.as_str()) };
        let Some(focus) = self.focus else {
            return (None, echo); // composition ran; delivery is focus-gated
        };
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
            (Some(outcome.preedit), Some(outcome.candidates))
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
mod tests {
    use super::*;

    fn focused() -> ImedCore {
        let mut core = ImedCore::new();
        core.set_focus(7, true, wire::FIELD_KIND_TEXT);
        core
    }

    fn key(core: &mut ImedCore, kind: u8, ch: u32, action: u8) -> Option<KeyPushes> {
        core.key(kind, ch, action).0
    }

    #[test]
    fn unfocused_keys_compose_but_deliver_nothing() {
        let mut core = ImedCore::new();
        let (pushes, echo) = core.key(wire::KEY_KIND_TEXT, u32::from('a'), 0);
        assert_eq!(pushes, None, "delivery is focus-gated");
        assert_eq!(echo.commit.as_str(), "a", "the probe echo sees the step");
    }

    #[test]
    fn plain_text_commits_directly() {
        let mut core = focused();
        let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('ä'), 0).unwrap();
        assert_eq!(push.surface_id, 7);
        assert_eq!(push.commit.unwrap().as_str(), "ä");
        assert_eq!(push.action, None);
    }

    #[test]
    fn dead_key_sequence_commits_composed_char() {
        let mut core = focused();
        assert_eq!(key(&mut core, wire::KEY_KIND_DEAD, u32::from('´'), 0), None);
        let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('e'), 0).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "é");
    }

    #[test]
    fn dead_key_fallback_commits_both_chars() {
        let mut core = focused();
        assert_eq!(key(&mut core, wire::KEY_KIND_DEAD, u32::from('´'), 0), None);
        let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('x'), 0).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "´x");
    }

    #[test]
    fn actions_pass_through_and_flush_pending() {
        let mut core = focused();
        let push = key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_BACKSPACE).unwrap();
        assert_eq!(push.action, Some(wire::ACTION_BACKSPACE));
        assert_eq!(push.commit, None);

        // Pending accent + Enter: commit the accent AND pass Enter through.
        assert_eq!(key(&mut core, wire::KEY_KIND_DEAD, u32::from('´'), 0), None);
        let push = key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_ENTER).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "´");
        assert_eq!(push.action, Some(wire::ACTION_ENTER));
    }

    #[test]
    fn focus_transition_cancels_pending_accent() {
        let mut core = focused();
        assert_eq!(key(&mut core, wire::KEY_KIND_DEAD, u32::from('´'), 0), None);
        core.set_focus(9, true, wire::FIELD_KIND_TEXT);
        let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('e'), 0).unwrap();
        assert_eq!(push.surface_id, 9);
        assert_eq!(push.commit.unwrap().as_str(), "e", "no ´ leaked across fields");
    }

    #[test]
    fn jp_layout_composes_and_pushes_preedit_then_candidates() {
        let mut core = focused();
        core.set_layout("jp");
        // "n" shows the romaji tail as preedit; "i" resolves it to に.
        let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('n'), 0).unwrap();
        assert_eq!(push.preedit.unwrap().as_str(), "n");
        let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('i'), 0).unwrap();
        assert_eq!(push.commit, None);
        assert_eq!(push.preedit.unwrap().as_str(), "に");
        // Enter commits the kana and CLEARS the strip (empty snapshots).
        let push = key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_ENTER).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "に");
        assert!(push.preedit.unwrap().is_empty());
    }

    #[test]
    fn candidate_select_commits_from_current_page() {
        let mut core = focused();
        core.set_layout("zh");
        for ch in "nihao".chars() {
            let _ = core.key(wire::KEY_KIND_TEXT, u32::from(ch), 0);
        }
        // Space opens candidates.
        let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from(' '), 0).unwrap();
        let cands = push.candidates.unwrap();
        assert_eq!(cands.get(0).map(|c| c.as_str()), Some("你好"));
        let (push, echo) = core.candidate_select(0);
        assert_eq!(push.unwrap().commit.unwrap().as_str(), "你好");
        assert_eq!(echo.commit.as_str(), "你好");
    }

    #[test]
    fn test_reject_password_fields_bypass_engine_and_strip() {
        let mut core = ImedCore::new();
        core.set_layout("jp");
        core.set_focus(7, true, wire::FIELD_KIND_PASSWORD);
        // Romaji is NOT composed in a password field — raw chars commit,
        // and no preedit/candidate snapshot is ever pushed.
        let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('n'), 0).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "n");
        assert_eq!(push.preedit, None);
        assert_eq!(push.candidates, None);
        let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('i'), 0).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "i");
    }

    #[test]
    fn layout_switch_resets_composition() {
        let mut core = focused();
        core.set_layout("jp");
        let _ = core.key(wire::KEY_KIND_TEXT, u32::from('n'), 0);
        core.set_layout("us");
        let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('i'), 0).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "i", "no romaji tail survived");
    }

    #[test]
    fn test_reject_malformed_key_kinds() {
        let mut core = focused();
        assert_eq!(key(&mut core, 99, u32::from('a'), 0), None);
        assert_eq!(key(&mut core, wire::KEY_KIND_TEXT, 0xD800, 0), None); // invalid scalar
        assert_eq!(key(&mut core, wire::KEY_KIND_ACTION, 0, 99), None); // unknown action
    }
}
