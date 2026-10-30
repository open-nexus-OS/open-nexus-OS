// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The shortcut registry (RFC-0113 §7, TASK-0346 P3) — every key chord the system acts
//! on, in ONE table compiled into the components that need it: inputd matches the window and
//! capture chords before a key enters the text route ([`system_action`]), the keymap matches the
//! field commands and the input-source switch ([`lookup`]), and the settings page lists it by
//! category (`svc.shortcuts.list`). A chord matches only with EXACTLY its modifiers. Names live
//! in the language files by id (`sc.<id>`); the key caps are named here ([`chord_label`]).
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/registry.rs`; the keymap side in
//!   `tests/input_v1_0_host/tests/keymaps_contract.rs`, inputd's in `tests/key_facts.rs`
//! ADR: docs/adr/0090-the-input-source-decides-the-route-windowd-orders-focus-and-keys-the-compositor-owns-the-caret.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod chord;
mod label;
mod listing;
mod table;

pub use chord::{Action, Category, Chord, EditKey, Key, Mods, Owner, Shortcut};
pub use label::{chord_label, chords_label};
pub use listing::{listing, Listed};
pub use table::SHORTCUTS;

use hid::KeyboardUsage;

/// The entry a press of `usage` is with exactly `mods` held. `letter` names the lowercase ASCII
/// letter the active layout types on the key (`Key::Letter`, `None` if it types none) — asked only
/// when a letter chord holds exactly these modifiers, so plain typing never pays for it. At most
/// one entry matches (`tests/registry.rs` keeps the chords unique).
#[must_use]
pub fn lookup(
    mods: Mods,
    usage: KeyboardUsage,
    letter: impl FnOnce() -> Option<char>,
) -> Option<&'static Shortcut> {
    let find = |key: Key| {
        SHORTCUTS.iter().find(|e| e.chords.iter().any(|c| c.mods == mods && c.key == key))
    };
    if let Some(entry) = find(Key::Usage(usage)) {
        return Some(entry);
    }
    let letters = |c: &Chord| c.mods == mods && matches!(c.key, Key::Letter(_));
    if !SHORTCUTS.iter().any(|e| e.chords.iter().any(letters)) {
        return None;
    }
    find(Key::Letter(letter()?))
}

/// inputd's match before a key enters the text route: a window command or a capture key. Field
/// commands and unbound keys go on to the keymap.
#[must_use]
pub fn system_action(
    mods: Mods,
    usage: KeyboardUsage,
    letter: impl FnOnce() -> Option<char>,
) -> Option<Action> {
    let entry = lookup(mods, usage, letter)?;
    matches!(entry.action, Action::Window(_) | Action::Capture(_)).then_some(entry.action)
}

/// One category's entries in table order (the settings page's list).
pub fn in_category(category: Category) -> impl Iterator<Item = &'static Shortcut> {
    SHORTCUTS.iter().filter(move |entry| entry.category == category)
}

/// The entry with this id.
#[must_use]
pub fn by_id(id: &str) -> Option<&'static Shortcut> {
    SHORTCUTS.iter().find(|entry| entry.id == id)
}
