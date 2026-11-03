// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The key caps' names for the settings page (`svc.shortcuts.list`). German names the
//! modifier and navigation keys as German keyboards print them; every other language uses the
//! Latin names its keyboards carry. Modifiers go Super, Ctrl, Alt, AltGr, Shift, then the key.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/registry.rs` (every chord of the table has a name in both sets)

use alloc::string::String;
use hid::KeyboardUsage as U;

use crate::chord::{Chord, Key, Mods};

struct Caps {
    ctrl: &'static str,
    shift: &'static str,
    home: &'static str,
    end: &'static str,
    delete: &'static str,
    backspace: &'static str,
    print: &'static str,
    space: &'static str,
    tab: &'static str,
}

const GERMAN: Caps = Caps {
    ctrl: "Strg",
    shift: "Umschalt",
    home: "Pos1",
    end: "Ende",
    delete: "Entf",
    backspace: "Rücktaste",
    print: "Druck",
    space: "Leertaste",
    tab: "Tab",
};

const LATIN: Caps = Caps {
    ctrl: "Ctrl",
    shift: "Shift",
    home: "Home",
    end: "End",
    delete: "Delete",
    backspace: "Backspace",
    print: "Print",
    space: "Space",
    tab: "Tab",
};

/// `lang` is a BCP-47-ish tag (`de-DE`, `de`, `en-US`, …); only its first subtag counts.
fn caps(lang: &str) -> &'static Caps {
    let primary = lang.split(['-', '_']).next().unwrap_or("");
    if primary.eq_ignore_ascii_case("de") {
        &GERMAN
    } else {
        &LATIN
    }
}

/// The name of a key the table uses; `None` = a usage no chord names (a test keeps every chord
/// of the table named).
fn key_name(key: Key, caps: &Caps, out: &mut String) -> Option<()> {
    match key {
        Key::Letter(letter) => out.push(letter.to_ascii_uppercase()),
        Key::Usage(usage) => out.push_str(match usage {
            U::LEFT_ARROW => "←",
            U::RIGHT_ARROW => "→",
            U::UP_ARROW => "↑",
            U::DOWN_ARROW => "↓",
            U::HOME => caps.home,
            U::END => caps.end,
            U::DELETE_FORWARD => caps.delete,
            U::BACKSPACE => caps.backspace,
            U::PRINT_SCREEN => caps.print,
            U::SPACE => caps.space,
            U::TAB => caps.tab,
            _ => return None,
        }),
    }
    Some(())
}

/// One chord as the keyboard names it in `lang`, e.g. `Super+Strg+Umschalt+←`; `None` when the
/// chord's key has no name here.
#[must_use]
pub fn chord_label(chord: &Chord, lang: &str) -> Option<String> {
    let caps = caps(lang);
    let mut out = String::new();
    for (bit, name) in [
        (Mods::SUPER, "Super"),
        (Mods::CTRL, caps.ctrl),
        (Mods::ALT, "Alt"),
        (Mods::ALTGR, "AltGr"),
        (Mods::SHIFT, caps.shift),
    ] {
        if chord.mods.contains(bit) {
            out.push_str(name);
            out.push('+');
        }
    }
    key_name(chord.key, caps, &mut out)?;
    Some(out)
}

/// Several chords, ` · ` between them (the page's line of alternates); `""` for none.
#[must_use]
pub fn chords_label(chords: &[Chord], lang: &str) -> Option<String> {
    let mut out = String::new();
    for (i, chord) in chords.iter().enumerate() {
        if i > 0 {
            out.push_str(" · ");
        }
        out.push_str(&chord_label(chord, lang)?);
    }
    Some(out)
}
