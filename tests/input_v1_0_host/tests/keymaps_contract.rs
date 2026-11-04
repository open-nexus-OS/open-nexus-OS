// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for shared base keymap authority across supported layouts.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 12 integration tests.
//!
//! TEST_SCOPE:
//!   - deterministic layout vectors for `us`, `de`, `jp`, `kr`, `zh`
//!   - IME-switch shared primitive behavior
//!   - unknown layout and unsupported key/modifier rejects
//!   - every field chord of the shortcut registry, in every layout, with exact modifiers
//!
//! TEST_SCENARIOS:
//!   - keymaps_resolve_deterministic_vectors_for_all_layouts()
//!   - test_reject_* layout, key, and modifier rejects
//!
//! DEPENDENCIES:
//!   - `hid::KeyboardUsage`
//!   - `keymaps` crate resolution API
//!   - `shortcuts` (the registry the keymap matches its field chords from)
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

/// TASK-0067B: the navigation keys are editing commands in every layout (Shift selects), and
/// Ctrl+A/C/X/V follow the layout's letter (falling back to the US key position where the
/// layout types no Latin letter); every other Ctrl combination stays refused.
#[test]
fn editing_commands_resolve_by_key_and_by_layout_letter() {
    use keymaps::{EditKey, KeyAction};
    let none = Modifiers::default();
    let shift = Modifiers::default().with_shift();
    let ctrl = Modifiers::default().with_control();
    let edit = |out: KeyOutput| match out {
        KeyOutput::Action(KeyAction::Edit(k)) => k,
        other => panic!("expected an editing command, got {other:?}"),
    };
    for layout in [LayoutId::Us, LayoutId::De, LayoutId::Jp, LayoutId::Kr, LayoutId::Zh] {
        let km = Keymap::new(layout);
        assert_eq!(edit(km.resolve(KeyboardUsage::LEFT_ARROW, none).expect("left")), EditKey::Left);
        assert_eq!(
            edit(km.resolve(KeyboardUsage::LEFT_ARROW, shift).expect("shift left")),
            EditKey::SelectLeft
        );
        assert_eq!(
            edit(km.resolve(KeyboardUsage::END, shift).expect("shift end")),
            EditKey::SelectEnd
        );
        assert_eq!(
            edit(km.resolve(KeyboardUsage::DELETE_FORWARD, none).expect("del")),
            EditKey::Delete
        );
        assert_eq!(
            edit(km.resolve(KeyboardUsage::A, ctrl).expect("ctrl a")),
            EditKey::SelectAll,
            "{layout:?}"
        );
        assert_eq!(
            edit(km.resolve(KeyboardUsage::C, ctrl).expect("ctrl c")),
            EditKey::Copy,
            "{layout:?}"
        );
        assert_eq!(
            edit(km.resolve(KeyboardUsage::X, ctrl).expect("ctrl x")),
            EditKey::Cut,
            "{layout:?}"
        );
        assert_eq!(
            edit(km.resolve(KeyboardUsage::V, ctrl).expect("ctrl v")),
            EditKey::Paste,
            "{layout:?}"
        );
    }
}

/// TASK-0346 P2: with Ctrl the arrows step by words (Shift selects), Backspace and Delete
/// delete a word, Ctrl+Z undoes and Ctrl+Shift+Z / Ctrl+Y redo — Z and Y by the layout's letter
/// (the German layout swaps the keys).
#[test]
fn word_steps_and_the_history_resolve_with_ctrl() {
    use keymaps::{EditKey, KeyAction};
    let ctrl = Modifiers::default().with_control();
    let ctrl_shift = Modifiers::default().with_control().with_shift();
    let edit = |out: KeyOutput| match out {
        KeyOutput::Action(KeyAction::Edit(k)) => k,
        other => panic!("expected an editing command, got {other:?}"),
    };
    let us = Keymap::new(LayoutId::Us);
    let pairs = [
        (KeyboardUsage::LEFT_ARROW, ctrl, EditKey::WordLeft),
        (KeyboardUsage::RIGHT_ARROW, ctrl, EditKey::WordRight),
        (KeyboardUsage::LEFT_ARROW, ctrl_shift, EditKey::SelectWordLeft),
        (KeyboardUsage::RIGHT_ARROW, ctrl_shift, EditKey::SelectWordRight),
        (KeyboardUsage::BACKSPACE, ctrl, EditKey::DeleteWordBack),
        (KeyboardUsage::DELETE_FORWARD, ctrl, EditKey::DeleteWordForward),
        (KeyboardUsage::Z, ctrl, EditKey::Undo),
        (KeyboardUsage::Z, ctrl_shift, EditKey::Redo),
        (KeyboardUsage::Y, ctrl, EditKey::Redo),
    ];
    for (usage, mods, want) in pairs {
        assert_eq!(edit(us.resolve(usage, mods).expect("resolves")), want, "{usage:?}");
    }
    // Shift+Tab walks back (its own action, not a Tab).
    assert_eq!(
        us.resolve(KeyboardUsage::TAB, Modifiers::default().with_shift()).expect("shift tab"),
        KeyOutput::Action(KeyAction::BackTab)
    );
    // German: the key in the US `Y` position types `z` — Ctrl on it is the undo.
    let de = Keymap::new(LayoutId::De);
    assert_eq!(edit(de.resolve(KeyboardUsage::Y, ctrl).expect("de ctrl z")), EditKey::Undo);
    assert_eq!(edit(de.resolve(KeyboardUsage::Z, ctrl).expect("de ctrl y")), EditKey::Redo);
}

#[test]
fn test_reject_other_ctrl_combinations() {
    let ctrl = Modifiers::default().with_control();
    let us = Keymap::new(LayoutId::Us);
    for usage in [KeyboardUsage::B, KeyboardUsage::Q, KeyboardUsage::DIGIT_1] {
        let err = us.resolve(usage, ctrl).unwrap_err();
        assert_eq!(err.code(), KeymapError::UnsupportedModifierCombination.code());
    }
    let ctrl_shift = Modifiers::default().with_control().with_shift();
    assert!(us.resolve(KeyboardUsage::C, ctrl_shift).is_err(), "Ctrl+Shift+C is no copy");
    assert!(us.resolve(KeyboardUsage::Y, ctrl_shift).is_err(), "Ctrl+Shift+Y is no redo");
    assert!(us.resolve(KeyboardUsage::BACKSPACE, ctrl_shift).is_err(), "the modifier set is exact");
}

fn every_layout() -> [Keymap; 5] {
    [LayoutId::Us, LayoutId::De, LayoutId::Jp, LayoutId::Kr, LayoutId::Zh].map(Keymap::new)
}

/// The keymap's Modifiers for a registry set (the inverse of `Modifiers::chord_mods`).
fn held(mods: shortcuts::Mods) -> Modifiers {
    let mut held = Modifiers::default();
    for (bit, with) in [
        (shortcuts::Mods::SHIFT, Modifiers::with_shift as fn(Modifiers) -> Modifiers),
        (shortcuts::Mods::CTRL, Modifiers::with_control),
        (shortcuts::Mods::ALT, Modifiers::with_alt),
        (shortcuts::Mods::ALTGR, Modifiers::with_alt_gr),
        (shortcuts::Mods::SUPER, Modifiers::with_super),
    ] {
        if mods.contains(bit) {
            held = with(held);
        }
    }
    assert_eq!(held.chord_mods(), mods);
    held
}

/// The key a chord is pressed on in `km`: by place, or the key that types its letter there.
fn key_of(km: &Keymap, key: shortcuts::Key) -> KeyboardUsage {
    match key {
        shortcuts::Key::Usage(usage) => usage,
        shortcuts::Key::Letter(letter) => (KeyboardUsage::A.raw()..=KeyboardUsage::Z.raw())
            .map(KeyboardUsage::from_raw)
            .find(|usage| km.letter(*usage) == Some(letter))
            .expect("every layout types every letter chord"),
    }
}

/// RFC-0113 §7: the keymap's field commands and its input-source switch ARE the registry's — every
/// such chord resolves to its entry's action in every layout.
#[test]
fn every_registry_field_chord_resolves_in_every_layout() {
    use shortcuts::{Action, SHORTCUTS};
    for km in every_layout() {
        for entry in SHORTCUTS {
            let want = match entry.action {
                Action::Field(key) => KeyOutput::Action(KeyAction::Edit(key)),
                Action::FocusNext => KeyOutput::Action(KeyAction::Tab),
                Action::FocusPrevious => KeyOutput::Action(KeyAction::BackTab),
                Action::NextInputSource => KeyOutput::Action(KeyAction::ImeSwitch),
                Action::Window(_) | Action::Capture(_) => continue,
            };
            for chord in entry.chords {
                let usage = key_of(&km, chord.key);
                let got = km.resolve(usage, held(chord.mods)).expect(entry.id);
                assert_eq!(got, want, "{} {km:?}", entry.id);
            }
        }
    }
}

/// The modifier set is exact: Alt or Super on top of a field chord makes it no command — and
/// Alt or Super alone keeps typing as before (they select no character).
#[test]
fn test_reject_field_chords_under_alt_or_super() {
    let us = Keymap::new(LayoutId::Us);
    let alt = Modifiers::default().with_alt();
    let sup = Modifiers::default().with_super();
    assert!(us.resolve(KeyboardUsage::LEFT_ARROW, alt).is_err(), "Alt+← moves no caret");
    assert!(us.resolve(KeyboardUsage::LEFT_ARROW, sup).is_err(), "Super+← moves no caret");
    let ctrl_alt = Modifiers::default().with_control().with_alt();
    assert!(us.resolve(KeyboardUsage::C, ctrl_alt).is_err(), "Ctrl+Alt+C is no copy");
    let ctrl_super = Modifiers::default().with_control().with_super();
    assert!(us.resolve(KeyboardUsage::V, ctrl_super).is_err(), "Super+Ctrl+V is no paste");
    assert_eq!(text(us.resolve(KeyboardUsage::A, alt).expect("alt a")), 'a');
}

/// A letter chord follows the layout's letter; where a layout types no Latin letter on a key,
/// its US letter counts. Non-letter keys have none.
#[test]
fn letters_follow_the_layout() {
    let de = Keymap::new(LayoutId::De);
    assert_eq!(de.letter(KeyboardUsage::Y), Some('z'), "QWERTZ swaps Y and Z");
    assert_eq!(de.letter(KeyboardUsage::Z), Some('y'));
    assert_eq!(de.letter(KeyboardUsage::LEFT_BRACKET), None, "ü is no Latin letter key");
    let us = Keymap::new(LayoutId::Us);
    assert_eq!(us.letter(KeyboardUsage::W), Some('w'));
    assert_eq!(us.letter(KeyboardUsage::LEFT_ARROW), None);
    assert_eq!(us.letter(KeyboardUsage::DIGIT_1), None);
}
