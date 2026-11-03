// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The registry's contract (RFC-0113 §7): one entry per chord, exact modifiers, every
//! chord named in both key-cap sets, every entry named in the settings page's five languages and
//! listed by its `ScName` component.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 11 tests.
//!
//! TEST_SCOPE:
//!   - chord uniqueness, exact modifier matching, the system/field split
//!   - key-cap names (German and Latin)
//!   - names in de/en/ja/ko/zh and the settings page's id → name arms
//!
//! TEST_SCENARIOS:
//!   - test_reject_a_shortcut_with_extra_modifiers()
//!   - test_reject_a_shortcut_with_a_missing_modifier()
//!   - every_chord_belongs_to_one_entry()

use hid::KeyboardUsage as U;
use shortcuts::{
    by_id, chord_label, chords_label, in_category, listing, lookup, system_action, Action,
    Category, Chord, EditKey, Key, Mods, SHORTCUTS,
};

/// The press a chord stands for: its usage and the letter its layout types (letters sit on
/// their US keys for the test — the keymap's own letter rule is the keymap's test).
fn press(chord: &Chord) -> (U, Option<char>) {
    match chord.key {
        Key::Usage(usage) => (usage, None),
        Key::Letter(letter) => (U::from_raw(U::A.raw() + (letter as u8 - b'a')), Some(letter)),
    }
}

const BITS: [Mods; 5] = [Mods::SHIFT, Mods::CTRL, Mods::ALT, Mods::ALTGR, Mods::SUPER];

#[test]
fn every_chord_belongs_to_one_entry() {
    let chords: Vec<(&str, Chord)> =
        SHORTCUTS.iter().flat_map(|e| e.chords.iter().map(move |c| (e.id, *c))).collect();
    for (i, (id, chord)) in chords.iter().enumerate() {
        for (other, again) in &chords[i + 1..] {
            assert_ne!(chord, again, "{id} and {other} share a chord");
        }
        match chord.key {
            Key::Letter(letter) => assert!(letter.is_ascii_lowercase(), "{id}: {letter:?}"),
            // A letter key goes by its letter, never by its place — the layout decides it.
            Key::Usage(usage) => assert!(
                !(U::A.raw()..=U::Z.raw()).contains(&usage.raw()),
                "{id}: a letter key by place"
            ),
        }
        let (usage, letter) = press(chord);
        assert_eq!(lookup(chord.mods, usage, || letter).map(|e| e.id), Some(*id));
    }
}

/// The entry that names exactly `mods` on this key, if one does.
fn owner_of(mods: Mods, key: Key) -> Option<&'static str> {
    SHORTCUTS.iter().find(|e| e.chords.iter().any(|c| c.mods == mods && c.key == key)).map(|e| e.id)
}

#[test]
fn test_reject_a_shortcut_with_extra_modifiers() {
    for entry in SHORTCUTS {
        for chord in entry.chords {
            let (usage, letter) = press(chord);
            for bit in BITS.into_iter().filter(|bit| !chord.mods.contains(*bit)) {
                // One more modifier is another chord: the entry that names that very set (an
                // alternate of the same entry, another entry, or none) — never this one by a subset.
                let held = chord.mods.with(bit);
                let hit = lookup(held, usage, || letter).map(|e| e.id);
                assert_eq!(hit, owner_of(held, chord.key), "{} with {held:?}", entry.id);
            }
        }
    }
    let wm = Mods::SUPER.with(Mods::CTRL);
    assert_eq!(lookup(wm.with(Mods::SHIFT), U::F, || Some('f')), None, "Super+Ctrl+Shift+F");
    let quarter = wm.with(Mods::SHIFT).with(Mods::ALT);
    assert_eq!(lookup(quarter, U::LEFT_ARROW, || None), None, "Shift and Alt at once");
    assert_eq!(system_action(Mods::CTRL, U::PRINT_SCREEN, || None), None, "Ctrl+Print stays free");
    assert_eq!(
        system_action(Mods::SUPER, U::PRINT_SCREEN, || None),
        None,
        "Super+Print stays free"
    );
    let shift_alt = Mods::SHIFT.with(Mods::ALT);
    assert_eq!(system_action(shift_alt, U::PRINT_SCREEN, || None), None, "Shift+Alt+Print");
    assert_eq!(lookup(Mods::ALT, U::LEFT_ARROW, || None), None, "Alt+← moves no caret");
}

#[test]
fn test_reject_a_shortcut_with_a_missing_modifier() {
    for entry in SHORTCUTS {
        for chord in entry.chords {
            let (usage, letter) = press(chord);
            for bit in BITS.into_iter().filter(|bit| chord.mods.contains(*bit)) {
                let held = Mods::from_bits(chord.mods.bits() & !bit.bits());
                let hit = lookup(held, usage, || letter).map(|e| e.id);
                assert_eq!(hit, owner_of(held, chord.key), "{} with {held:?}", entry.id);
            }
        }
    }
    // Ctrl alone on a window letter is no window command (and Ctrl+W is nothing at all).
    assert_eq!(lookup(Mods::CTRL, U::W, || Some('w')), None);
}

#[test]
fn inputd_takes_the_window_and_capture_chords_only() {
    let wm = Mods::SUPER.with(Mods::CTRL);
    assert_eq!(system_action(wm, U::LEFT_ARROW, || None), Some(Action::Window(1)));
    assert_eq!(system_action(wm, U::F, || Some('f')), Some(Action::Window(9)));
    assert_eq!(system_action(wm, U::UP_ARROW, || None), Some(Action::Window(9)));
    assert_eq!(system_action(wm, U::W, || Some('w')), Some(Action::Window(15)));
    assert_eq!(system_action(Mods::NONE, U::PRINT_SCREEN, || None), Some(Action::Capture(1)));
    assert_eq!(system_action(Mods::SHIFT, U::PRINT_SCREEN, || None), Some(Action::Capture(2)));
    assert_eq!(system_action(Mods::ALT, U::PRINT_SCREEN, || None), Some(Action::Capture(3)));
    // Field commands and the input-source switch are the keymap's.
    assert_eq!(system_action(Mods::CTRL, U::C, || Some('c')), None);
    assert_eq!(system_action(Mods::NONE, U::LEFT_ARROW, || None), None);
    assert_eq!(system_action(Mods::CTRL, U::SPACE, || None), None);
    // The focus moves are the keymap's (Tab / Shift+Tab), listed with the text commands.
    assert_eq!(system_action(Mods::NONE, U::TAB, || None), None);
    assert_eq!(lookup(Mods::NONE, U::TAB, || None).map(|e| e.action), Some(Action::FocusNext));
    assert_eq!(lookup(Mods::SHIFT, U::TAB, || None).map(|e| e.action), Some(Action::FocusPrevious));
    assert_eq!(lookup(Mods::ALT, U::TAB, || None), None, "Alt+Tab stays free");
    let copy = lookup(Mods::CTRL, U::C, || Some('c')).expect("ctrl c");
    assert_eq!(copy.action, Action::Field(EditKey::Copy));
}

/// Plain and shifted typing never asks the layout for a letter (no letter chord holds those sets).
#[test]
fn plain_typing_never_asks_for_the_letter() {
    let asked = || -> Option<char> { panic!("the letter was asked") };
    assert_eq!(lookup(Mods::NONE, U::A, asked).map(|e| e.id), None);
    assert_eq!(lookup(Mods::SHIFT, U::A, asked).map(|e| e.id), None);
    assert_eq!(lookup(Mods::NONE, U::HOME, asked).map(|e| e.id), Some("text.home"));
}

#[test]
fn every_window_and_capture_entry_is_acted_on() {
    for entry in SHORTCUTS {
        if matches!(
            entry.action,
            Action::Window(_)
                | Action::Capture(_)
                | Action::Field(_)
                | Action::FocusNext
                | Action::FocusPrevious
        ) {
            assert!(entry.active, "{}: its owner acts on every one", entry.id);
        }
    }
    assert!(!by_id("input.nextSource").expect("listed").active, "nothing switches yet");
}

#[test]
fn ids_name_their_category_and_the_page_lists_each_once() {
    let mut listed = 0;
    for category in Category::ALL {
        assert_eq!(Category::from_id(category.id()), Some(category));
        let prefix = match category {
            Category::Windows => "window.",
            Category::Screenshots => "capture.",
            Category::Text => "text.",
            Category::Input => "input.",
        };
        let entries: Vec<_> = in_category(category).collect();
        assert!(!entries.is_empty(), "{category:?} is empty");
        for entry in &entries {
            assert!(entry.id.starts_with(prefix), "{} is not {prefix}*", entry.id);
            assert!(!entry.chords.is_empty(), "{} has no chord", entry.id);
            assert_eq!(by_id(entry.id).map(|e| e.action), Some(entry.action));
        }
        listed += entries.len();
    }
    assert_eq!(listed, SHORTCUTS.len());
    assert_eq!(Category::from_id("bogus"), None);
}

#[test]
fn key_caps_are_named_in_german_and_latin() {
    let top_left = by_id("window.topLeft").expect("listed").chords[0];
    assert_eq!(chord_label(&top_left, "de-DE").as_deref(), Some("Super+Strg+Umschalt+←"));
    assert_eq!(chord_label(&top_left, "en-US").as_deref(), Some("Super+Ctrl+Shift+←"));
    let space = by_id("input.nextSource").expect("listed").chords[0];
    assert_eq!(chord_label(&space, "de").as_deref(), Some("Strg+Leertaste"));
    assert_eq!(chord_label(&space, "ja-JP").as_deref(), Some("Ctrl+Space"));
    assert_eq!(chord_label(&space, "").as_deref(), Some("Ctrl+Space"));
    let redo = by_id("text.redo").expect("listed");
    assert_eq!(chords_label(&redo.chords[1..], "de").as_deref(), Some("Strg+Y"));
    assert_eq!(
        chords_label(by_id("window.fill").expect("listed").chords, "en").as_deref(),
        Some("Super+Ctrl+↑ · Super+Ctrl+F")
    );
    assert_eq!(chords_label(&[], "de").as_deref(), Some(""));
    for entry in SHORTCUTS {
        for lang in ["de", "en"] {
            let label = chords_label(entry.chords, lang);
            assert!(label.is_some_and(|l| !l.is_empty()), "{} unnamed in {lang}", entry.id);
        }
    }
    let unnamed = Chord { mods: Mods::NONE, key: Key::Usage(U::F1) };
    assert_eq!(chord_label(&unnamed, "en"), None);
}

/// The page's rows: one per entry of the category, in table order, named for the language; an
/// unknown category lists nothing.
#[test]
fn the_listing_names_each_row_for_the_page() {
    let rows = listing("text", "de-DE");
    let ids: Vec<&str> = rows.iter().map(|r| r.id).collect();
    let want: Vec<&str> = in_category(Category::Text).map(|e| e.id).collect();
    assert_eq!(ids, want);
    let redo = rows.iter().find(|r| r.id == "text.redo").expect("redo listed");
    assert_eq!((redo.keys.as_str(), redo.alt.as_str()), ("Strg+Umschalt+Z", "Strg+Y"));
    let copy = rows.iter().find(|r| r.id == "text.copy").expect("copy listed");
    assert_eq!((copy.keys.as_str(), copy.alt.as_str(), copy.active), ("Strg+C", "", true));
    let input = listing("input", "en");
    assert_eq!(input.len(), 1);
    assert_eq!((input[0].keys.as_str(), input[0].active), ("Ctrl+Space", false));
    assert!(listing("bogus", "en").is_empty());
    let total: usize = Category::ALL.iter().map(|c| listing(c.id(), "en").len()).sum();
    assert_eq!(total, SHORTCUTS.len());
}

#[test]
fn mods_keep_the_wire_bits() {
    assert_eq!(Mods::from_bits(0xff).bits(), 0x1f, "bits no modifier names are dropped");
    let held = Mods::SUPER.with(Mods::CTRL);
    assert!(held.contains(Mods::CTRL) && !held.contains(Mods::SHIFT));
    assert_eq!(held.bits(), 18);
}

/// Names come from the settings page's language files by id (`sc.<id>`), and its `ScName`
/// component has the arm that shows each — `@t` takes literal keys only.
#[test]
fn every_entry_is_named_in_five_languages_and_on_the_page() {
    let app = concat!(env!("CARGO_MANIFEST_DIR"), "/../apps/settings");
    let read =
        |path: String| std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let names = read(format!("{app}/ui/components/shortcuts/ScName.nx"));
    for lang in ["de", "en", "ja", "ko", "zh"] {
        let file = read(format!("{app}/i18n/{lang}.json"));
        for entry in SHORTCUTS {
            assert!(file.contains(&format!("\"sc.{}\":", entry.id)), "{lang}: sc.{}", entry.id);
        }
        for category in Category::ALL {
            let key = format!("\"sc.cat.{}\":", category.id());
            assert!(file.contains(&key), "{lang}: {key}");
        }
    }
    for entry in SHORTCUTS {
        assert!(names.contains(&format!("== \"{}\"", entry.id)), "ScName misses {}", entry.id);
        assert!(names.contains(&format!("@t(\"sc.{}\")", entry.id)), "ScName: sc.{}", entry.id);
    }
}
