// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0075 dead-key/compose state machine — deterministic, bounded,
//! const-table driven. DE dead keys `´` `` ` `` `^` for Phase 0.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable for RFC-0075 Phase 0
//! TEST_COVERAGE: `tests/compose_contract.rs` (sequences, fallback, cancel, bounds).
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use crate::outcome::{Commit, ImeAction, ImeKey, ImeOutcome};

/// Maximum pending dead keys (RFC-0075 bounds compose sequences; the Latin
/// machine holds at most one pending accent).
pub const COMPOSE_PENDING_MAX: usize = 1;

/// `(dead, base) → composed` pairs. Order is irrelevant (exact match), the
/// table is const and exhaustive for the shipped DE dead keys.
const COMPOSE_TABLE: &[(char, char, char)] = &[
    // acute ´
    ('´', 'a', 'á'),
    ('´', 'e', 'é'),
    ('´', 'i', 'í'),
    ('´', 'o', 'ó'),
    ('´', 'u', 'ú'),
    ('´', 'y', 'ý'),
    ('´', 'A', 'Á'),
    ('´', 'E', 'É'),
    ('´', 'I', 'Í'),
    ('´', 'O', 'Ó'),
    ('´', 'U', 'Ú'),
    ('´', 'Y', 'Ý'),
    // grave `
    ('`', 'a', 'à'),
    ('`', 'e', 'è'),
    ('`', 'i', 'ì'),
    ('`', 'o', 'ò'),
    ('`', 'u', 'ù'),
    ('`', 'A', 'À'),
    ('`', 'E', 'È'),
    ('`', 'I', 'Ì'),
    ('`', 'O', 'Ò'),
    ('`', 'U', 'Ù'),
    // circumflex ^
    ('^', 'a', 'â'),
    ('^', 'e', 'ê'),
    ('^', 'i', 'î'),
    ('^', 'o', 'ô'),
    ('^', 'u', 'û'),
    ('^', 'A', 'Â'),
    ('^', 'E', 'Ê'),
    ('^', 'I', 'Î'),
    ('^', 'O', 'Ô'),
    ('^', 'U', 'Û'),
];

fn compose(dead: char, base: char) -> Option<char> {
    COMPOSE_TABLE
        .iter()
        .find(|(d, b, _)| *d == dead && *b == base)
        .map(|(_, _, composed)| *composed)
}

/// Deterministic Latin dead-key composer.
///
/// Semantics (normative, RFC-0075):
/// - `Dead(d)` with no pending accent arms `d` (no output).
/// - `Dead(d)` with pending `p`: same accent twice commits the accent itself;
///   a different accent commits `p` standalone and arms `d`.
/// - `Text(c)` with pending `p`: table hit commits the composed character,
///   miss falls back to committing `p` then `c` (never swallowed).
/// - `Escape`/`Backspace` with pending: cancels the pending accent (consumed).
/// - Other actions with pending: commit the accent standalone, pass the
///   action through (`pass_action`).
/// - Without pending state, keys pass through unhandled.
#[derive(Debug, Clone, Copy, Default)]
pub struct Composer {
    pending: Option<char>,
}

impl Composer {
    #[must_use]
    pub const fn new() -> Self {
        Self { pending: None }
    }

    #[must_use]
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Cancels any pending accent (focus loss, surface switch).
    pub fn reset(&mut self) {
        self.pending = None;
    }

    /// Feeds one key; returns what happened. Deterministic and total —
    /// every `ImeKey` has a defined outcome in every state.
    pub fn feed(&mut self, key: ImeKey) -> ImeOutcome {
        match (self.pending.take(), key) {
            (None, ImeKey::Dead(dead)) => {
                self.pending = Some(dead);
                ImeOutcome { handled: true, ..ImeOutcome::default() }
            }
            (None, _) => ImeOutcome::default(),
            (Some(pending), ImeKey::Dead(dead)) => {
                if pending == dead {
                    ImeOutcome { handled: true, commit: Commit::one(pending), ..Default::default() }
                } else {
                    self.pending = Some(dead);
                    ImeOutcome { handled: true, commit: Commit::one(pending), ..Default::default() }
                }
            }
            (Some(pending), ImeKey::Text(ch)) => {
                let commit = match compose(pending, ch) {
                    Some(composed) => Commit::one(composed),
                    None => Commit::two(pending, ch),
                };
                ImeOutcome { handled: true, commit, ..Default::default() }
            }
            (Some(_), ImeKey::Action(ImeAction::Escape | ImeAction::Backspace)) => {
                ImeOutcome { handled: true, ..ImeOutcome::default() }
            }
            (Some(pending), ImeKey::Action(action)) => ImeOutcome {
                handled: true,
                commit: Commit::one(pending),
                pass_action: Some(action),
            },
        }
    }
}
