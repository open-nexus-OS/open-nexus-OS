// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for shared base keymap authority across supported layouts.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 4 integration tests.
//!
//! TEST_SCOPE:
//!   - deterministic layout vectors for `us`, `de`, `jp`, `kr`, `zh`
//!   - IME-switch shared primitive behavior
//!   - unknown layout and unsupported key/modifier rejects
//!
//! TEST_SCENARIOS:
//!   - keymaps_resolve_deterministic_vectors_for_all_layouts()
//!   - test_reject_* layout, key, and modifier rejects
//!
//! DEPENDENCIES:
//!   - `hid::KeyboardUsage`
//!   - `keymaps` crate resolution API
//!
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use hid::KeyboardUsage;
use keymaps::{KeyAction, KeyOutput, Keymap, KeymapError, LayoutId, Modifiers};

fn text(output: KeyOutput) -> char {
    match output {
        KeyOutput::Text(ch) => ch,
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn keymaps_resolve_deterministic_vectors_for_all_layouts() {
    let none = Modifiers::default();
    let shift = Modifiers::default().with_shift();
    let alt_gr = Modifiers::default().with_alt_gr();
    let ctrl = Modifiers::default().with_control();

    let us = Keymap::new(LayoutId::try_from("us").expect("us"));
    assert_eq!(text(us.resolve(KeyboardUsage::A, none).expect("us a")), 'a');
    assert_eq!(text(us.resolve(KeyboardUsage::A, shift).expect("us shift a")), 'A');
    assert_eq!(text(us.resolve(KeyboardUsage::DIGIT_2, shift).expect("us @")), '@');

    let de = Keymap::new(LayoutId::try_from("de").expect("de"));
    assert_eq!(text(de.resolve(KeyboardUsage::LEFT_BRACKET, none).expect("de ue")), 'ü');
    assert_eq!(text(de.resolve(KeyboardUsage::LEFT_BRACKET, shift).expect("de Ue")), 'Ü');
    assert_eq!(text(de.resolve(KeyboardUsage::SEMICOLON, none).expect("de oe")), 'ö');
    assert_eq!(text(de.resolve(KeyboardUsage::APOSTROPHE, none).expect("de ae")), 'ä');
    assert_eq!(text(de.resolve(KeyboardUsage::Q, alt_gr).expect("de altgr q")), '@');
    assert_eq!(text(de.resolve(KeyboardUsage::E, alt_gr).expect("de altgr e")), '€');

    let jp = Keymap::new(LayoutId::try_from("jp").expect("jp"));
    assert_eq!(text(jp.resolve(KeyboardUsage::A, none).expect("jp a")), 'a');
    assert_eq!(text(jp.resolve(KeyboardUsage::DIGIT_2, shift).expect("jp quote")), '"');
    assert_eq!(text(jp.resolve(KeyboardUsage::DIGIT_7, shift).expect("jp apostrophe")), '\'');

    let kr = Keymap::new(LayoutId::try_from("kr").expect("kr"));
    assert_eq!(text(kr.resolve(KeyboardUsage::A, none).expect("kr a")), 'a');
    assert_eq!(text(kr.resolve(KeyboardUsage::DIGIT_2, shift).expect("kr @")), '@');
    assert_eq!(text(kr.resolve(KeyboardUsage::BACKSLASH, none).expect("kr won")), '₩');
    assert_eq!(text(kr.resolve(KeyboardUsage::BACKSLASH, shift).expect("kr pipe")), '|');
    assert_eq!(
        kr.resolve(KeyboardUsage::SPACE, ctrl).expect("kr ime switch"),
        KeyOutput::Action(KeyAction::ImeSwitch)
    );

    let zh = Keymap::new(LayoutId::try_from("zh").expect("zh"));
    assert_eq!(text(zh.resolve(KeyboardUsage::A, none).expect("zh a")), 'a');
    assert_eq!(text(zh.resolve(KeyboardUsage::SLASH, none).expect("zh slash")), '/');
    assert_eq!(text(zh.resolve(KeyboardUsage::NON_US_HASH, none).expect("zh yuan")), '￥');
    assert_eq!(text(zh.resolve(KeyboardUsage::NON_US_HASH, shift).expect("zh pipe")), '|');
    assert_eq!(
        zh.resolve(KeyboardUsage::SPACE, ctrl).expect("zh ime switch"),
        KeyOutput::Action(KeyAction::ImeSwitch)
    );
}

#[test]
fn keymaps_de_marks_dead_keys() {
    let none = Modifiers::default();
    let shift = Modifiers::default().with_shift();

    let de = Keymap::new(LayoutId::try_from("de").expect("de"));
    assert_eq!(de.resolve(KeyboardUsage::EQUAL, none).expect("de acute"), KeyOutput::Dead('´'));
    assert_eq!(de.resolve(KeyboardUsage::EQUAL, shift).expect("de grave"), KeyOutput::Dead('`'));
    assert_eq!(
        de.resolve(KeyboardUsage::GRAVE, none).expect("de circumflex"),
        KeyOutput::Dead('^')
    );
    // Shifted GRAVE stays plain text (degree sign, not a dead key).
    assert_eq!(text(de.resolve(KeyboardUsage::GRAVE, shift).expect("de degree")), '°');

    // US has no dead keys: the same physical keys emit plain text.
    let us = Keymap::new(LayoutId::try_from("us").expect("us"));
    assert_eq!(text(us.resolve(KeyboardUsage::EQUAL, none).expect("us equal")), '=');
    assert_eq!(text(us.resolve(KeyboardUsage::GRAVE, none).expect("us backtick")), '`');
}

#[test]
fn test_reject_unknown_layout_id() {
    let err = LayoutId::try_from("neo").unwrap_err();
    assert_eq!(err.code(), "keymap.layout.unknown");
}

#[test]
fn test_reject_unsupported_key_usage() {
    let us = Keymap::new(LayoutId::Us);
    let err = us.resolve(KeyboardUsage::F1, Modifiers::default()).unwrap_err();
    assert_eq!(err.code(), "keymap.key.unsupported");
}

#[test]
fn test_reject_alt_gr_on_layout_without_support() {
    let us = Keymap::new(LayoutId::Us);
    let err = us.resolve(KeyboardUsage::Q, Modifiers::default().with_alt_gr()).unwrap_err();
    assert_eq!(err.code(), KeymapError::UnsupportedModifierCombination.code());
}

#[test]
fn osk_rows_are_data_complete_and_aligned() {
    use keymaps::{osk_rows, LayoutId, OSK_ROWS};
    // Every layout serves all rows; row 0 is always the digit row; every
    // text key has label+key, every action key has an action (never both).
    for layout in [LayoutId::Us, LayoutId::De, LayoutId::Jp, LayoutId::Kr, LayoutId::Zh] {
        for row in 0..OSK_ROWS {
            let keys = osk_rows(layout, row);
            assert!(!keys.is_empty(), "{layout:?} row {row}");
            for k in keys {
                if k.action.is_empty() {
                    assert!(!k.label.is_empty() && !k.key.is_empty());
                } else {
                    assert!(k.key.is_empty(), "action key must not carry a text key");
                }
            }
        }
        assert_eq!(osk_rows(layout, 0)[0].key, "1");
        // Out-of-range rows are EMPTY (the app renders nothing) — bounded.
        assert!(osk_rows(layout, OSK_ROWS).is_empty());
    }
    // KR shows jamo labels over 2-set Latin keys (label ≠ key by design).
    let kr = osk_rows(LayoutId::Kr, 1);
    assert_eq!((kr[0].label, kr[0].key), ("ㅂ", "q"));
    // DE carries the umlaut tail + ß; JP/ZH share the us rows (the engine
    // converts romaji/pinyin — data, not per-language branches).
    assert_eq!(osk_rows(LayoutId::De, 1).last().unwrap().key, "ü");
    assert_eq!(osk_rows(LayoutId::Jp, 1), osk_rows(LayoutId::Us, 1));
    assert_eq!(osk_rows(LayoutId::Zh, 2), osk_rows(LayoutId::Us, 2));
}
