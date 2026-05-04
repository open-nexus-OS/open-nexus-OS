// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Layout selection and resolution logic for shared base keymap authority.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/keymaps_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use hid::KeyboardUsage;

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
        if modifiers.control() {
            if usage == KeyboardUsage::SPACE && !modifiers.alt_gr() {
                return Ok(KeyOutput::Action(KeyAction::ImeSwitch));
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
