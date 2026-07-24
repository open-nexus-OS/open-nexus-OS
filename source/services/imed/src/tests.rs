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
