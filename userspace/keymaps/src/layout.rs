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

use crate::{
    table::{self, lookup, MappingEntry},
    EditKey, KeyAction, KeyOutput, KeymapError, Modifiers,
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
        // The navigation keys edit a text field in every layout; Shift selects.
        if !modifiers.control() && !modifiers.alt_gr() {
            if let Some(key) = navigation(usage, modifiers.shift()) {
                return Ok(KeyOutput::Action(KeyAction::Edit(key)));
            }
        }
        if modifiers.control() {
            if usage == KeyboardUsage::SPACE && !modifiers.alt_gr() {
                return Ok(KeyOutput::Action(KeyAction::ImeSwitch));
            }
            if !modifiers.alt_gr() && !modifiers.shift() {
                if let Some(key) = self.shortcut(usage) {
                    return Ok(KeyOutput::Action(KeyAction::Edit(key)));
                }
            }
            return Err(KeymapError::UnsupportedModifierCombination);
        }
        if modifiers.shift() && modifiers.alt_gr() {
            return Err(KeymapError::UnsupportedModifierCombination);
        }

        let tables = match self.layout {
            LayoutId::Us => Tables::Borrowed(table::us::TABLE),
            LayoutId::De => Tables::Borrowed(table::de::TABLE),
            LayoutId::Jp => Tables::Owned(table::jp::merged()),
            LayoutId::Kr => Tables::Owned(table::kr::merged()),
            LayoutId::Zh => Tables::Owned(table::zh::merged()),
        };
        let entry = lookup(tables.as_slice(), usage).ok_or(KeymapError::UnsupportedKey)?;
        resolve_entry(self.layout, entry, modifiers)
    }
}

impl Keymap {
    /// Ctrl+A / C / X / V by the LAYOUT's letter (Ctrl+C is the key that types `c` in the
    /// active layout, as on every desktop); a layout whose key types no Latin letter there
    /// falls back to the key's US position, so the shortcuts work in every layout.
    fn shortcut(&self, usage: KeyboardUsage) -> Option<EditKey> {
        let letter = match self.resolve(usage, Modifiers::default()) {
            Ok(KeyOutput::Text(ch)) if ch.is_ascii_alphabetic() => ch.to_ascii_lowercase(),
            _ => match usage {
                KeyboardUsage::A => 'a',
                KeyboardUsage::C => 'c',
                KeyboardUsage::X => 'x',
                KeyboardUsage::V => 'v',
                _ => return None,
            },
        };
        match letter {
            'a' => Some(EditKey::SelectAll),
            'c' => Some(EditKey::Copy),
            'x' => Some(EditKey::Cut),
            'v' => Some(EditKey::Paste),
            _ => None,
        }
    }
}

/// The navigation keys as editing commands (layout-independent); Shift extends a selection.
fn navigation(usage: KeyboardUsage, shift: bool) -> Option<EditKey> {
    Some(match (usage, shift) {
        (KeyboardUsage::LEFT_ARROW, false) => EditKey::Left,
        (KeyboardUsage::RIGHT_ARROW, false) => EditKey::Right,
        (KeyboardUsage::HOME, false) => EditKey::Home,
        (KeyboardUsage::END, false) => EditKey::End,
        (KeyboardUsage::LEFT_ARROW, true) => EditKey::SelectLeft,
        (KeyboardUsage::RIGHT_ARROW, true) => EditKey::SelectRight,
        (KeyboardUsage::HOME, true) => EditKey::SelectHome,
        (KeyboardUsage::END, true) => EditKey::SelectEnd,
        (KeyboardUsage::DELETE_FORWARD, _) => EditKey::Delete,
        _ => return None,
    })
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
