// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Layout selection and resolution logic for shared base keymap authority.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/keymaps_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use alloc::vec::Vec;
use hid::KeyboardUsage;

use shortcuts::Action;

use crate::{
    table::{self, lookup, MappingEntry},
    KeyAction, KeyOutput, KeymapError, Modifiers,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutId {
    Us,
    De,
    Jp,
    Kr,
    Zh,
}

impl TryFrom<&str> for LayoutId {
    type Error = KeymapError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.trim().to_ascii_lowercase().as_str() {
            "us" => Ok(Self::Us),
            "de" => Ok(Self::De),
            "jp" => Ok(Self::Jp),
            "kr" => Ok(Self::Kr),
            "zh" => Ok(Self::Zh),
            _ => Err(KeymapError::UnknownLayout),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Keymap {
    layout: LayoutId,
}

impl Keymap {
    #[must_use]
    pub const fn new(layout: LayoutId) -> Self {
        Self { layout }
    }

    pub fn resolve(
        &self,
        usage: KeyboardUsage,
        modifiers: Modifiers,
    ) -> Result<KeyOutput, KeymapError> {
        let tables = self.tables();
        // The shortcut registry first (RFC-0113 §7): the field commands and the input-source
        // switch, each only with exactly its modifiers. Window and capture chords never get here —
        // inputd takes them before the keymap.
        let letter = || letter_in(tables.as_slice(), usage);
        if let Some(entry) = shortcuts::lookup(modifiers.chord_mods(), usage, letter) {
            match entry.action {
                Action::Field(key) => return Ok(KeyOutput::Action(KeyAction::Edit(key))),
                Action::NextInputSource => return Ok(KeyOutput::Action(KeyAction::ImeSwitch)),
                Action::Window(_) | Action::Capture(_) => {}
            }
        }
        if modifiers.control() || (modifiers.shift() && modifiers.alt_gr()) {
            return Err(KeymapError::UnsupportedModifierCombination);
        }
        let entry = lookup(tables.as_slice(), usage).ok_or(KeymapError::UnsupportedKey)?;
        resolve_entry(self.layout, entry, modifiers)
    }

    /// The lowercase ASCII letter this layout types on `usage` — where it types no Latin letter
    /// there, the key's US letter — so a letter chord (`shortcuts::Key::Letter`: Ctrl+C is the key
    /// that types `c`) works in every layout. `None` for a key that is no letter.
    #[must_use]
    pub fn letter(&self, usage: KeyboardUsage) -> Option<char> {
        letter_in(self.tables().as_slice(), usage)
    }

    fn tables(&self) -> Tables {
        match self.layout {
            LayoutId::Us => Tables::Borrowed(table::us::TABLE),
            LayoutId::De => Tables::Borrowed(table::de::TABLE),
            LayoutId::Jp => Tables::Owned(table::jp::merged()),
            LayoutId::Kr => Tables::Owned(table::kr::merged()),
            LayoutId::Zh => Tables::Owned(table::zh::merged()),
        }
    }
}

fn letter_in(tables: &[MappingEntry], usage: KeyboardUsage) -> Option<char> {
    match lookup(tables, usage).map(|entry| entry.base) {
        Some(KeyOutput::Text(ch)) if ch.is_ascii_alphabetic() => Some(ch.to_ascii_lowercase()),
        _ => {
            let place = usage.raw().checked_sub(KeyboardUsage::A.raw())?;
            (usage.raw() <= KeyboardUsage::Z.raw()).then(|| char::from(b'a' + place))
        }
    }
}

enum Tables {
    Borrowed(&'static [MappingEntry]),
    Owned(Vec<MappingEntry>),
}

impl Tables {
    fn as_slice(&self) -> &[MappingEntry] {
        match self {
            Self::Borrowed(slice) => slice,
            Self::Owned(entries) => entries.as_slice(),
        }
    }
}

fn resolve_entry(
    layout: LayoutId,
    entry: &MappingEntry,
    modifiers: Modifiers,
) -> Result<KeyOutput, KeymapError> {
    if modifiers.alt_gr() {
        return entry.alt_gr.ok_or(match layout {
            LayoutId::De => KeymapError::UnsupportedKey,
            LayoutId::Us | LayoutId::Jp | LayoutId::Kr | LayoutId::Zh => {
                KeymapError::UnsupportedModifierCombination
            }
        });
    }
    if modifiers.shift() {
        return entry.shifted.ok_or(KeymapError::UnsupportedModifierCombination);
    }
    Ok(entry.base)
}
