// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The provider tables: what a clock/reset id of a provider means in registers.
//! Ids are the binding header's (`config/board/include/dt-bindings`); offsets and
//! bits are transcribed facts (the mainline driver documentation, the board's
//! measured state — see each table's provenance line). Never an address.

pub mod k1;

use crate::field::Field;
use crate::provider::ProviderKind;

/// A parent a mux can select: its documented name and fixed rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parent {
    pub name: &'static str,
    pub hz: u64,
}

/// One clock of a provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockEntry {
    pub provider: ProviderKind,
    pub id: u32,
    pub name: &'static str,
    /// Register (offset in the provider's window) holding gate, mux and div.
    pub reg: u16,
    /// Gate mask; set = on. 0 = ungated.
    pub gate: u32,
    /// Parent select and post-divider (`rate = parent / (div + 1)`, inferred from
    /// the measured 375 MHz eMMC clock at div field 0).
    pub mux: Option<Field>,
    pub div: Option<Field>,
    /// The frequency-change bit (set after a mux/div write, self-clears) and the
    /// register it lives in (the "split" clocks keep it in another register).
    pub fc: u32,
    pub fc_reg: u16,
    pub parents: &'static [Parent],
    /// The one parent of a gate-only clock (rate reported from it), else 0 Hz.
    pub fixed_parent_hz: u64,
}

/// One reset line of a provider: `mask` in `reg`; `assert_sets` = writing the
/// bit asserts (APBC/APBC2), else writing the bit RELEASES (APMU).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetEntry {
    pub provider: ProviderKind,
    pub id: u32,
    pub name: &'static str,
    pub reg: u16,
    pub mask: u32,
    pub assert_sets: bool,
}

impl ResetEntry {
    /// The register value that means "released" for this line's bit.
    pub const fn released_bits(&self) -> u32 {
        if self.assert_sets {
            0
        } else {
            self.mask
        }
    }
}

/// Look a clock up by provider and id.
pub fn clock(provider: ProviderKind, id: u32) -> Option<&'static ClockEntry> {
    k1::CLOCKS.iter().find(|c| c.provider == provider && c.id == id)
}

/// Look a reset up by provider and id.
pub fn reset(provider: ProviderKind, id: u32) -> Option<&'static ResetEntry> {
    k1::RESETS.iter().find(|r| r.provider == provider && r.id == id)
}
