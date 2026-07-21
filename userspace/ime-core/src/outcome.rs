// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0075 IME key/outcome types — the deterministic value contract
//! between key resolution (keymaps), composition (ime-core) and delivery (imed).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable for RFC-0075 Phase 0
//! TEST_COVERAGE: Covered via `tests/compose_contract.rs`.
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use keymaps::{KeyAction, KeyOutput};

/// Maximum preedit size in bytes (RFC-0075 bound).
pub const PREEDIT_MAX_BYTES: usize = 64;

/// Editing actions the IME cares about (subset of `keymaps::KeyAction`;
/// `ImeSwitch` is handled upstream by inputd and never reaches composition).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeAction {
    Enter,
    Escape,
    Backspace,
    Tab,
}

/// A resolved key entering the composition machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeKey {
    Text(char),
    Dead(char),
    Action(ImeAction),
}

impl ImeKey {
    /// Maps a keymap resolution onto the IME key space.
    /// Returns `None` for keys composition never consumes (`ImeSwitch`).
    #[must_use]
    pub fn from_key_output(output: KeyOutput) -> Option<Self> {
        match output {
            KeyOutput::Text(ch) => Some(Self::Text(ch)),
            KeyOutput::Dead(ch) => Some(Self::Dead(ch)),
            KeyOutput::Action(KeyAction::Enter) => Some(Self::Action(ImeAction::Enter)),
            KeyOutput::Action(KeyAction::Escape) => Some(Self::Action(ImeAction::Escape)),
            KeyOutput::Action(KeyAction::Backspace) => Some(Self::Action(ImeAction::Backspace)),
            KeyOutput::Action(KeyAction::Tab) => Some(Self::Action(ImeAction::Tab)),
            KeyOutput::Action(KeyAction::ImeSwitch) => None,
        }
    }
}

/// Committed text output of one composition step (bounded: dead-key fallback
/// emits at most the standalone accent plus the following character).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Commit {
    chars: [Option<char>; 2],
}

impl Commit {
    #[must_use]
    pub const fn empty() -> Self {
        Self { chars: [None, None] }
    }

    #[must_use]
    pub const fn one(ch: char) -> Self {
        Self { chars: [Some(ch), None] }
    }

    #[must_use]
    pub const fn two(first: char, second: char) -> Self {
        Self { chars: [Some(first), Some(second)] }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chars[0].is_none()
    }

    /// Iterates the committed characters in order.
    pub fn chars(&self) -> impl Iterator<Item = char> + '_ {
        self.chars.iter().flatten().copied()
    }
}

/// Bounded preedit buffer (composition-in-progress text; used by CJK engines,
/// TASK-0149 — Latin dead-key composition commits directly and keeps this empty).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preedit {
    bytes: [u8; PREEDIT_MAX_BYTES],
    len: u8,
}

impl Default for Preedit {
    fn default() -> Self {
        Self::empty()
    }
}

impl Preedit {
    #[must_use]
    pub const fn empty() -> Self {
        Self { bytes: [0; PREEDIT_MAX_BYTES], len: 0 }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends a character; returns `false` (unchanged) when the bound is hit.
    pub fn push(&mut self, ch: char) -> bool {
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf).as_bytes();
        let len = usize::from(self.len);
        if len + encoded.len() > PREEDIT_MAX_BYTES {
            return false;
        }
        self.bytes[len..len + encoded.len()].copy_from_slice(encoded);
        self.len = (len + encoded.len()) as u8;
        true
    }

    /// Removes the last character; returns `false` when already empty.
    pub fn pop(&mut self) -> bool {
        let text = self.as_str();
        let Some((idx, _)) = text.char_indices().next_back() else {
            return false;
        };
        self.len = idx as u8;
        true
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        // The buffer only ever holds bytes written by `encode_utf8` at
        // char boundaries, so this cannot fail; fail closed regardless.
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

/// Result of feeding one key into the composition machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImeOutcome {
    /// True when composition consumed the key (it must not be delivered
    /// downstream as a raw action/text).
    pub handled: bool,
    /// Committed text produced by this step (may be empty).
    pub commit: Commit,
    /// Action to pass through downstream (set when composition flushed
    /// pending state but the action itself still applies, e.g. Enter).
    pub pass_action: Option<ImeAction>,
}
