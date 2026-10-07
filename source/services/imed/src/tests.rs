// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for the imed focus/key/composition state machine + the
//! TASK-0204 personalization integration (train/rank/persist/toggle gating).

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

/// TASK-0074 D3: window focus without a field (`FIELD_KIND_NONE`) delivers
/// Escape to the surface — how an app-modal hears ESC with no text field.
#[test]
fn surface_focus_without_a_field_passes_escape_only() {
    let mut core = ImedCore::new();
    core.set_focus(9, true, wire::FIELD_KIND_NONE);
    let push = key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_ESCAPE).unwrap();
    assert_eq!(push.surface_id, 9);
    assert_eq!(push.action, Some(wire::ACTION_ESCAPE));
    assert_eq!(push.commit, None);
    assert_eq!(push.preedit, None);
    assert_eq!(push.candidates, None);
}

/// …and NOTHING else reaches it: no text, no other action, no learning.
#[test]
fn test_reject_surface_focus_without_a_field_never_delivers_text() {
    let mut core = ImedCore::new();
    core.set_focus(9, true, wire::FIELD_KIND_NONE);
    assert_eq!(key(&mut core, wire::KEY_KIND_TEXT, u32::from('a'), 0), None);
    assert_eq!(key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_ENTER), None);
    assert_eq!(key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_BACKSPACE), None);
    assert_eq!(core.learned_count(), 0, "no learning without a field");
    // A real field afterwards works as before.
    core.set_focus(9, true, wire::FIELD_KIND_TEXT);
    assert!(key(&mut core, wire::KEY_KIND_TEXT, u32::from('a'), 0).is_some());
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

// ——— TASK-0204: personalization learning + persistence integration ———

/// In-memory `BlobIo` for the load/flush round-trip test.
struct FakeIo(std::collections::BTreeMap<String, Vec<u8>>);
impl BlobIo for FakeIo {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }
    fn write(&mut self, path: &str, bytes: &[u8]) -> bool {
        self.0.insert(path.to_string(), bytes.to_vec());
        true
    }
}

/// A composed commit routed through `plan()` (dead key `´` + `e` → `é`) is
/// learned by the personalization store.
#[test]
fn text_field_commit_trains() {
    let mut core = focused();
    assert_eq!(key(&mut core, wire::KEY_KIND_DEAD, u32::from('´'), 0), None);
    let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('e'), 0).unwrap();
    assert_eq!(push.commit.unwrap().as_str(), "é");
    assert_eq!(core.learned_count(), 1, "the committed candidate is learned");
}

/// Security invariant: a PASSWORD field never trains — the password bypass
/// commits directly without routing through `plan()`.
#[test]
fn password_field_never_trains() {
    let mut core = ImedCore::new();
    core.set_focus(7, true, wire::FIELD_KIND_PASSWORD);
    let _ = key(&mut core, wire::KEY_KIND_DEAD, u32::from('´'), 0);
    let _ = key(&mut core, wire::KEY_KIND_TEXT, u32::from('e'), 0);
    assert_eq!(core.learned_count(), 0, "password fields never learn");
}

/// Learned words survive a flush → reload (the statefs shape, faked here).
#[test]
fn learned_words_persist_across_reload() {
    let mut io = FakeIo(std::collections::BTreeMap::new());
    let mut core = focused();
    let _ = key(&mut core, wire::KEY_KIND_DEAD, u32::from('´'), 0);
    let _ = key(&mut core, wire::KEY_KIND_TEXT, u32::from('e'), 0);
    assert!(core.flush_store(&mut io), "a dirty store flushes");

    let mut restored = focused();
    restored.load_store(&io);
    assert_eq!(restored.learned_count(), 1, "learned words survive reload");
}

/// Privacy invariant: `ime.personalization = off` disables learning (a
/// committed candidate is NOT stored), and drops any prior learning.
#[test]
fn toggle_off_disables_learning() {
    let mut core = focused();
    core.set_personalization(false);
    assert!(!core.personalization_enabled());
    let _ = key(&mut core, wire::KEY_KIND_DEAD, u32::from('´'), 0);
    let _ = key(&mut core, wire::KEY_KIND_TEXT, u32::from('e'), 0);
    assert_eq!(core.learned_count(), 0, "personalization off = no learning");
}

/// "Forget learned words" clears the store but keeps personalization enabled.
#[test]
fn forget_clears_learned_words() {
    let mut core = focused();
    let _ = key(&mut core, wire::KEY_KIND_DEAD, u32::from('´'), 0);
    let _ = key(&mut core, wire::KEY_KIND_TEXT, u32::from('e'), 0);
    assert_eq!(core.learned_count(), 1);
    core.forget_learned();
    assert_eq!(core.learned_count(), 0, "forget clears all learned words");
    assert!(core.personalization_enabled(), "forget keeps personalization on");
}

/// Relay-ordering guard (RFC-0075 Phase 8b): a stale in-flight relay of an
/// OLDER tag must not clobber a fresher OSK-originated switch — the exact
/// race that turned `set zh; type nihao` into a de-engine echo in QEMU.
#[test]
fn stale_relay_does_not_clobber_pending_switch() {
    let mut core = ImedCore::new();
    core.set_layout("zh");
    core.note_persisted("zh");
    assert!(!core.relay_layout("de"), "stale relay must be ignored");
    assert_eq!(core.layout_tag(), "zh", "engine stays on the fresh switch");
    assert!(!core.relay_layout("zh"), "own echo consumes the guard, no re-switch");
    assert!(core.relay_layout("de"), "post-echo relays are external changes and apply");
    assert_eq!(core.layout_tag(), "de");
}

/// The ignore budget self-terminates: even if our own echo never arrives
/// (coalesced away), an external change lands within the bound.
#[test]
fn relay_ignore_budget_self_terminates() {
    let mut core = ImedCore::new();
    core.set_layout("zh");
    core.note_persisted("zh");
    for _ in 0..4 {
        assert!(!core.relay_layout("jp"), "within budget: stale relays ignored");
        assert_eq!(core.layout_tag(), "zh");
    }
    assert!(core.relay_layout("jp"), "budget exhausted: the relay applies");
    assert_eq!(core.layout_tag(), "jp", "external change can never be starved");
}

#[test]
fn insert_commits_the_text_alone_into_the_focused_field() {
    let mut core = focused();
    // A half-typed composition is dropped first; the strip clears if it showed.
    let _ = key(&mut core, wire::KEY_KIND_TEXT, u32::from('a'), 0);
    let (pushes, echo) = core.insert("Hallo Welt");
    let pushes = pushes.expect("focused: delivers");
    assert_eq!(pushes.surface_id, 7);
    assert_eq!(pushes.commit.map(|c| c.as_str().to_string()), Some("Hallo Welt".to_string()));
    assert_eq!(echo.commit.as_str(), "Hallo Welt");
    assert!(pushes.action.is_none());
}

#[test]
fn test_reject_insert_without_text_focus() {
    let mut core = ImedCore::new();
    let (pushes, echo) = core.insert("x");
    assert!(pushes.is_none(), "no focus: nothing delivered");
    assert_eq!(echo.commit.as_str(), "x", "the step's commit is echoed like a typed key's");
    core.set_focus(9, true, wire::FIELD_KIND_NONE);
    assert!(core.insert("x").0.is_none(), "window focus without a field: nothing delivered");
    let mut core = focused();
    assert!(core.insert("").0.is_none(), "empty text: nothing delivered");
}

#[test]
fn editing_commands_pass_to_the_focused_field() {
    let mut core = focused();
    for action in
        [wire::ACTION_LEFT, wire::ACTION_SELECT_ALL, wire::ACTION_COPY, wire::ACTION_PASTE]
    {
        let push = key(&mut core, wire::KEY_KIND_ACTION, 0, action).expect("delivered");
        assert_eq!((push.surface_id, push.action, push.commit), (7, Some(action), None));
    }
}

#[test]
fn an_editing_command_commits_the_running_composition_first() {
    let mut core = focused();
    core.set_layout("jp");
    let _ = key(&mut core, wire::KEY_KIND_TEXT, u32::from('k'), 0);
    let push = key(&mut core, wire::KEY_KIND_TEXT, u32::from('a'), 0).expect("composing");
    assert_eq!(push.preedit.map(|p| p.as_str().to_string()), Some("か".to_string()));
    // Ctrl+C now: the composed か reaches the field BEFORE the copy acts, the strip clears.
    let push = key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_COPY).expect("delivered");
    assert_eq!(push.commit.map(|c| c.as_str().to_string()), Some("か".to_string()));
    assert_eq!(push.action, Some(wire::ACTION_COPY), "the command, not the Enter that committed");
    assert!(push.preedit.is_some_and(|p| p.is_empty()), "the strip is cleared");
}

#[test]
fn test_reject_copy_and_cut_out_of_a_password_field() {
    let mut core = ImedCore::new();
    core.set_focus(7, true, wire::FIELD_KIND_PASSWORD);
    assert!(key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_COPY).is_none());
    assert!(key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_CUT).is_none());
    // Moving the caret and pasting INTO a password field stay allowed.
    assert!(key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_PASTE).is_some());
    assert!(key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_LEFT).is_some());
}

#[test]
fn test_reject_editing_commands_without_a_text_field() {
    let mut core = ImedCore::new();
    assert!(
        key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_SELECT_ALL).is_none(),
        "no focus"
    );
    core.set_focus(9, true, wire::FIELD_KIND_NONE);
    assert!(key(&mut core, wire::KEY_KIND_ACTION, 0, wire::ACTION_PASTE).is_none(), "window only");
}
