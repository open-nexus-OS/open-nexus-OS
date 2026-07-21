// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0075 Phase 0 contract tests — deterministic dead-key/compose
//! sequences, fallback, cancellation, preedit bounds.
//! OWNERS: @ui
//! STATUS: Functional
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use ime_core::{Commit, Composer, ImeAction, ImeKey, ImeOutcome, Preedit};
use keymaps::{KeyAction, KeyOutput};

fn commits(outcome: ImeOutcome) -> String {
    outcome.commit.chars().collect()
}

#[test]
fn dead_key_composes_accented_characters() {
    let mut composer = Composer::new();

    let armed = composer.feed(ImeKey::Dead('´'));
    assert!(armed.handled);
    assert!(armed.commit.is_empty());
    assert!(composer.has_pending());

    let outcome = composer.feed(ImeKey::Text('e'));
    assert!(outcome.handled);
    assert_eq!(commits(outcome), "é");
    assert!(!composer.has_pending());

    composer.feed(ImeKey::Dead('^'));
    assert_eq!(commits(composer.feed(ImeKey::Text('a'))), "â");

    composer.feed(ImeKey::Dead('`'));
    assert_eq!(commits(composer.feed(ImeKey::Text('U'))), "Ù");
}

#[test]
fn unmatched_compose_falls_back_to_both_characters() {
    let mut composer = Composer::new();
    composer.feed(ImeKey::Dead('´'));
    let outcome = composer.feed(ImeKey::Text('x'));
    assert!(outcome.handled);
    assert_eq!(commits(outcome), "´x");
    assert!(!composer.has_pending());
}

#[test]
fn same_dead_key_twice_commits_the_accent_itself() {
    let mut composer = Composer::new();
    composer.feed(ImeKey::Dead('´'));
    let outcome = composer.feed(ImeKey::Dead('´'));
    assert_eq!(commits(outcome), "´");
    assert!(!composer.has_pending());
}

#[test]
fn different_dead_key_flushes_and_rearms() {
    let mut composer = Composer::new();
    composer.feed(ImeKey::Dead('´'));
    let outcome = composer.feed(ImeKey::Dead('^'));
    assert_eq!(commits(outcome), "´");
    assert!(composer.has_pending());
    assert_eq!(commits(composer.feed(ImeKey::Text('e'))), "ê");
}

#[test]
fn escape_and_backspace_cancel_pending_accent() {
    for cancel in [ImeAction::Escape, ImeAction::Backspace] {
        let mut composer = Composer::new();
        composer.feed(ImeKey::Dead('^'));
        let outcome = composer.feed(ImeKey::Action(cancel));
        assert!(outcome.handled, "{cancel:?} must consume the pending accent");
        assert!(outcome.commit.is_empty());
        assert_eq!(outcome.pass_action, None);
        assert!(!composer.has_pending());
        // Follow-up text is untouched by the cancelled accent.
        assert!(!composer.feed(ImeKey::Text('a')).handled);
    }
}

#[test]
fn enter_flushes_accent_and_passes_through() {
    let mut composer = Composer::new();
    composer.feed(ImeKey::Dead('´'));
    let outcome = composer.feed(ImeKey::Action(ImeAction::Enter));
    assert!(outcome.handled);
    assert_eq!(commits(outcome), "´");
    assert_eq!(outcome.pass_action, Some(ImeAction::Enter));
}

#[test]
fn keys_pass_through_without_pending_state() {
    let mut composer = Composer::new();
    assert!(!composer.feed(ImeKey::Text('a')).handled);
    assert!(!composer.feed(ImeKey::Action(ImeAction::Enter)).handled);
    assert!(!composer.feed(ImeKey::Action(ImeAction::Backspace)).handled);
}

#[test]
fn reset_cancels_pending_on_focus_loss() {
    let mut composer = Composer::new();
    composer.feed(ImeKey::Dead('´'));
    composer.reset();
    assert!(!composer.has_pending());
    assert!(!composer.feed(ImeKey::Text('e')).handled);
}

#[test]
fn ime_key_maps_keymap_outputs() {
    assert_eq!(ImeKey::from_key_output(KeyOutput::Text('ä')), Some(ImeKey::Text('ä')));
    assert_eq!(ImeKey::from_key_output(KeyOutput::Dead('^')), Some(ImeKey::Dead('^')));
    assert_eq!(
        ImeKey::from_key_output(KeyOutput::Action(KeyAction::Enter)),
        Some(ImeKey::Action(ImeAction::Enter))
    );
    assert_eq!(ImeKey::from_key_output(KeyOutput::Action(KeyAction::ImeSwitch)), None);
}

#[test]
fn deterministic_sequence_de_fixture() {
    // The RFC-0075 host fixture: "´e ^a `o ´x" over a fresh composer.
    let script = [
        ImeKey::Dead('´'),
        ImeKey::Text('e'),
        ImeKey::Dead('^'),
        ImeKey::Text('a'),
        ImeKey::Dead('`'),
        ImeKey::Text('o'),
        ImeKey::Dead('´'),
        ImeKey::Text('x'),
    ];
    for _ in 0..2 {
        let mut composer = Composer::new();
        let mut out = String::new();
        for key in script {
            out.extend(composer.feed(key).commit.chars());
        }
        assert_eq!(out, "éâò´x");
    }
}

#[test]
fn preedit_is_bounded_and_pops_by_character() {
    let mut preedit = Preedit::empty();
    assert!(preedit.push('あ'));
    assert!(preedit.push('ä'));
    assert_eq!(preedit.as_str(), "あä");
    assert!(preedit.pop());
    assert_eq!(preedit.as_str(), "あ");

    // Bound: 64 bytes — 3-byte chars fit 21 times (63 bytes), the 22nd fails.
    let mut full = Preedit::empty();
    for _ in 0..21 {
        assert!(full.push('あ'));
    }
    assert!(!full.push('あ'));
    assert!(full.push('a'), "single-byte char still fits the final byte");
    assert!(!full.push('b'));

    full.clear();
    assert!(full.is_empty());
    assert!(!full.pop());
}

#[test]
fn commit_iterates_in_order() {
    let commit = Commit::two('´', 'x');
    let text: String = commit.chars().collect();
    assert_eq!(text, "´x");
    assert!(Commit::empty().is_empty());
}
