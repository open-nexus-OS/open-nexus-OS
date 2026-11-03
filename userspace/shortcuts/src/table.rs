// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: THE shortcut table (RFC-0113 §7) — every key chord the system acts on. inputd
//! matches the window and capture chords before a key enters the text route; the keymap matches
//! the field commands and the input-source switch; the settings page lists the table by category.
//! Adding a shortcut is adding a row here plus its name in the language files (`sc.<id>`).
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/registry.rs` (unique chords, exact modifiers, names in five languages)

use hid::KeyboardUsage as U;

use crate::chord::{Action, Category, Chord, EditKey, Key, Mods, Shortcut};

const NONE: Mods = Mods::NONE;
const SHIFT: Mods = Mods::SHIFT;
const CTRL: Mods = Mods::CTRL;
const CTRL_SHIFT: Mods = Mods::CTRL.with(Mods::SHIFT);
const ALT: Mods = Mods::ALT;
/// The window modifier pair (TASK-0066).
const WM: Mods = Mods::SUPER.with(Mods::CTRL);
const WM_SHIFT: Mods = WM.with(Mods::SHIFT);
const WM_ALT: Mods = WM.with(Mods::ALT);

const fn key(mods: Mods, usage: U) -> Chord {
    Chord { mods, key: Key::Usage(usage) }
}

const fn letter(mods: Mods, letter: char) -> Chord {
    Chord { mods, key: Key::Letter(letter) }
}

const fn window(id: &'static str, code: u8, chords: &'static [Chord]) -> Shortcut {
    Shortcut { id, category: Category::Windows, action: Action::Window(code), chords, active: true }
}

const fn capture(id: &'static str, kind: u8, chords: &'static [Chord]) -> Shortcut {
    Shortcut {
        id,
        category: Category::Screenshots,
        action: Action::Capture(kind),
        chords,
        active: true,
    }
}

const fn field(id: &'static str, command: EditKey, chords: &'static [Chord]) -> Shortcut {
    Shortcut { id, category: Category::Text, action: Action::Field(command), chords, active: true }
}

/// A focus move between fields (the keymap's `Tab` / `BackTab`), listed with the text commands.
const fn focus(id: &'static str, action: Action, chords: &'static [Chord]) -> Shortcut {
    Shortcut { id, category: Category::Text, action, chords, active: true }
}

/// The registry, in the order the page lists each category.
pub const SHORTCUTS: &[Shortcut] = &[
    // Windows (windowd's `zones::CODE_*`).
    window("window.leftHalf", 1, &[key(WM, U::LEFT_ARROW)]),
    window("window.rightHalf", 2, &[key(WM, U::RIGHT_ARROW)]),
    window("window.topLeft", 5, &[key(WM_SHIFT, U::LEFT_ARROW)]),
    window("window.topRight", 6, &[key(WM_SHIFT, U::RIGHT_ARROW)]),
    window("window.bottomLeft", 7, &[key(WM_ALT, U::LEFT_ARROW)]),
    window("window.bottomRight", 8, &[key(WM_ALT, U::RIGHT_ARROW)]),
    window("window.fill", 9, &[key(WM, U::UP_ARROW), letter(WM, 'f')]),
    window("window.return", 10, &[key(WM, U::DOWN_ARROW), letter(WM, 'r')]),
    window("window.minimize", 14, &[letter(WM, 'm')]),
    window("window.close", 15, &[letter(WM, 'w')]),
    // RFC-0112: the compositor ends itself and init brings it back — every app window returns.
    window("window.restartDesktop", 16, &[key(WM_SHIFT, U::BACKSPACE)]),
    // Screenshots (RFC-0095). Print with Ctrl or Super held stays free.
    capture("capture.ui", 1, &[key(NONE, U::PRINT_SCREEN)]),
    capture("capture.screen", 2, &[key(SHIFT, U::PRINT_SCREEN)]),
    capture("capture.window", 3, &[key(ALT, U::PRINT_SCREEN)]),
    // Text fields: the focus between them, then moving, selecting, deleting, the clipboard, the
    // history. Tab and Shift+Tab live here alone — the keymap tables carry no Tab row.
    focus("text.focusNext", Action::FocusNext, &[key(NONE, U::TAB)]),
    focus("text.focusPrevious", Action::FocusPrevious, &[key(SHIFT, U::TAB)]),
    field("text.left", EditKey::Left, &[key(NONE, U::LEFT_ARROW)]),
    field("text.right", EditKey::Right, &[key(NONE, U::RIGHT_ARROW)]),
    field("text.wordLeft", EditKey::WordLeft, &[key(CTRL, U::LEFT_ARROW)]),
    field("text.wordRight", EditKey::WordRight, &[key(CTRL, U::RIGHT_ARROW)]),
    field("text.home", EditKey::Home, &[key(NONE, U::HOME), key(CTRL, U::HOME)]),
    field("text.end", EditKey::End, &[key(NONE, U::END), key(CTRL, U::END)]),
    field("text.selectLeft", EditKey::SelectLeft, &[key(SHIFT, U::LEFT_ARROW)]),
    field("text.selectRight", EditKey::SelectRight, &[key(SHIFT, U::RIGHT_ARROW)]),
    field("text.selectWordLeft", EditKey::SelectWordLeft, &[key(CTRL_SHIFT, U::LEFT_ARROW)]),
    field("text.selectWordRight", EditKey::SelectWordRight, &[key(CTRL_SHIFT, U::RIGHT_ARROW)]),
    field("text.selectHome", EditKey::SelectHome, &[key(SHIFT, U::HOME), key(CTRL_SHIFT, U::HOME)]),
    field("text.selectEnd", EditKey::SelectEnd, &[key(SHIFT, U::END), key(CTRL_SHIFT, U::END)]),
    field("text.selectAll", EditKey::SelectAll, &[letter(CTRL, 'a')]),
    // Shift+Delete too: Shift still held after a Shift+arrow selection deletes it.
    field(
        "text.delete",
        EditKey::Delete,
        &[key(NONE, U::DELETE_FORWARD), key(SHIFT, U::DELETE_FORWARD)],
    ),
    field("text.deleteWordBack", EditKey::DeleteWordBack, &[key(CTRL, U::BACKSPACE)]),
    field("text.deleteWordForward", EditKey::DeleteWordForward, &[key(CTRL, U::DELETE_FORWARD)]),
    field("text.copy", EditKey::Copy, &[letter(CTRL, 'c')]),
    field("text.cut", EditKey::Cut, &[letter(CTRL, 'x')]),
    field("text.paste", EditKey::Paste, &[letter(CTRL, 'v')]),
    field("text.undo", EditKey::Undo, &[letter(CTRL, 'z')]),
    field("text.redo", EditKey::Redo, &[letter(CTRL_SHIFT, 'z'), letter(CTRL, 'y')]),
    // Input sources. Matched (Ctrl+Space types no space) but nothing switches on it yet.
    Shortcut {
        id: "input.nextSource",
        category: Category::Input,
        action: Action::NextInputSource,
        chords: &[key(CTRL, U::SPACE)],
        active: false,
    },
];
