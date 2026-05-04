// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stable reject taxonomy for layout selection and modifier/keymap resolution.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/keymaps_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapError {
    UnknownLayout,
    UnsupportedKey,
    UnsupportedModifierCombination,
}

impl KeymapError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownLayout => "keymap.layout.unknown",
            Self::UnsupportedKey => "keymap.key.unsupported",
            Self::UnsupportedModifierCombination => "keymap.modifiers.unsupported",
        }
    }
}

impl fmt::Display for KeymapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownLayout => f.write_str("unknown keymap layout"),
            Self::UnsupportedKey => f.write_str("unsupported key usage for keymap"),
            Self::UnsupportedModifierCombination => {
                f.write_str("unsupported modifier combination for keymap")
            }
        }
    }
}

impl std::error::Error for KeymapError {}
