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

use ime_core::{Commit, Composer, ImeAction, ImeKey};
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

/// Committed text for one push — bounded to one Latin composition step
/// (composed char or dead-key fallback pair; ≤ 2 chars ≤ 8 UTF-8 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommitText {
    bytes: [u8; 8],
    len: u8,
}

impl CommitText {
    fn from_commit(commit: &Commit) -> Self {
        let mut out = Self::default();
        for ch in commit.chars() {
            let mut buf = [0u8; 4];
            let encoded = ch.encode_utf8(&mut buf).as_bytes();
            let len = usize::from(out.len);
            // Bounded by construction (≤ 2 chars); guard anyway, fail-closed.
            if len + encoded.len() > out.bytes.len() {
                break;
            }
            out.bytes[len..len + encoded.len()].copy_from_slice(encoded);
            out.len = (len + encoded.len()) as u8;
        }
        out
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyPushes {
    pub surface_id: u64,
    pub commit: Option<CommitText>,
    pub action: Option<u8>,
}

/// The IME state machine: windowd-relayed focus gates key processing;
/// ime-core composes; outputs are push plans for the os_lite layer.
#[derive(Debug, Default)]
pub struct ImedCore {
    composer: Composer,
    focus: Option<FocusState>,
}

impl ImedCore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn focus(&self) -> Option<FocusState> {
        self.focus
    }

    /// Applies a windowd focus relay. Any focus TRANSITION cancels pending
    /// composition state (a half-typed accent never leaks across fields).
    pub fn set_focus(&mut self, surface_id: u64, focused: bool, field_kind: u8) {
        let next = if focused { Some(FocusState { surface_id, field_kind }) } else { None };
        if next != self.focus {
            self.composer.reset();
        }
        self.focus = next;
    }

    /// Feeds one resolved key (wire `KEY_KIND_*`/`ACTION_*` vocabulary).
    /// Returns `None` when unfocused (keys are DROPPED — imed is the gate;
    /// inputd forwards unconditionally) or when the key only armed state.
    pub fn key(&mut self, kind: u8, ch: u32, action: u8) -> Option<KeyPushes> {
        let focus = self.focus?;
        let key = decode_key(kind, ch, action)?;
        let outcome = self.composer.feed(key);
        if outcome.handled {
            let commit =
                (!outcome.commit.is_empty()).then(|| CommitText::from_commit(&outcome.commit));
            let action = outcome.pass_action.map(encode_action);
            if commit.is_none() && action.is_none() {
                return None; // dead key armed — nothing to push yet
            }
            return Some(KeyPushes { surface_id: focus.surface_id, commit, action });
        }
        // Composition did not consume the key: text commits directly,
        // actions pass through unchanged.
        match key {
            ImeKey::Text(ch) => Some(KeyPushes {
                surface_id: focus.surface_id,
                commit: Some(CommitText::from_commit(&Commit::one(ch))),
                action: None,
            }),
            ImeKey::Action(act) => Some(KeyPushes {
                surface_id: focus.surface_id,
                commit: None,
                action: Some(encode_action(act)),
            }),
            ImeKey::Dead(_) => None,
        }
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

    #[test]
    fn unfocused_keys_are_dropped() {
        let mut core = ImedCore::new();
        assert_eq!(core.key(wire::KEY_KIND_TEXT, u32::from('a'), 0), None);
    }

    #[test]
    fn plain_text_commits_directly() {
        let mut core = focused();
        let push = core.key(wire::KEY_KIND_TEXT, u32::from('ä'), 0).unwrap();
        assert_eq!(push.surface_id, 7);
        assert_eq!(push.commit.unwrap().as_str(), "ä");
        assert_eq!(push.action, None);
    }

    #[test]
    fn dead_key_sequence_commits_composed_char() {
        let mut core = focused();
        assert_eq!(core.key(wire::KEY_KIND_DEAD, u32::from('´'), 0), None);
        let push = core.key(wire::KEY_KIND_TEXT, u32::from('e'), 0).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "é");
    }

    #[test]
    fn dead_key_fallback_commits_both_chars() {
        let mut core = focused();
        assert_eq!(core.key(wire::KEY_KIND_DEAD, u32::from('´'), 0), None);
        let push = core.key(wire::KEY_KIND_TEXT, u32::from('x'), 0).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "´x");
    }

    #[test]
    fn actions_pass_through_and_flush_pending() {
        let mut core = focused();
        let push = core.key(wire::KEY_KIND_ACTION, 0, wire::ACTION_BACKSPACE).unwrap();
        assert_eq!(push.action, Some(wire::ACTION_BACKSPACE));
        assert_eq!(push.commit, None);

        // Pending accent + Enter: commit the accent AND pass Enter through.
        assert_eq!(core.key(wire::KEY_KIND_DEAD, u32::from('´'), 0), None);
        let push = core.key(wire::KEY_KIND_ACTION, 0, wire::ACTION_ENTER).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "´");
        assert_eq!(push.action, Some(wire::ACTION_ENTER));
    }

    #[test]
    fn escape_cancels_pending_without_pushes() {
        let mut core = focused();
        assert_eq!(core.key(wire::KEY_KIND_DEAD, u32::from('^'), 0), None);
        assert_eq!(core.key(wire::KEY_KIND_ACTION, 0, wire::ACTION_ESCAPE), None);
        let push = core.key(wire::KEY_KIND_TEXT, u32::from('a'), 0).unwrap();
        assert_eq!(push.commit.unwrap().as_str(), "a", "accent was cancelled");
    }

    #[test]
    fn focus_transition_cancels_pending_accent() {
        let mut core = focused();
        assert_eq!(core.key(wire::KEY_KIND_DEAD, u32::from('´'), 0), None);
        core.set_focus(9, true, wire::FIELD_KIND_PASSWORD);
        let push = core.key(wire::KEY_KIND_TEXT, u32::from('e'), 0).unwrap();
        assert_eq!(push.surface_id, 9);
        assert_eq!(push.commit.unwrap().as_str(), "e", "no ´ leaked across fields");
    }

    #[test]
    fn test_reject_malformed_key_kinds() {
        let mut core = focused();
        assert_eq!(core.key(99, u32::from('a'), 0), None);
        assert_eq!(core.key(wire::KEY_KIND_TEXT, 0xD800, 0), None); // invalid scalar
        assert_eq!(core.key(wire::KEY_KIND_ACTION, 0, 99), None); // unknown action
    }
}
