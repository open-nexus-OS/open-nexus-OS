// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The provider tables: what a clock, reset or power-domain id of a provider means in registers.
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

impl ClockEntry {
    /// The mux and divider field values that make exactly `hz`: the first parent in mux order
    /// that a divider inside the field turns into `hz`, the smallest divider first. `None`
    /// when the clock has no mux or divider, or when no parent reaches `hz` exactly — a rate
    /// the tree demands is met exactly or refused, never rounded.
    pub fn select(&self, hz: u64) -> Option<(u32, u32)> {
        let (mux, div) = (self.mux?, self.div?);
        let muxes = 1usize << mux.width.min(8);
        let divs = 1u64 << div.width.min(8);
        for (m, parent) in self.parents.iter().enumerate().take(muxes) {
            for d in 0..divs {
                let n = d + 1;
                if parent.hz % n == 0 && parent.hz / n == hz {
                    return Some((m as u32, d as u32));
                }
            }
        }
        None
    }
}

/// One power domain of a provider: how it comes up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainEntry {
    pub provider: ProviderKind,
    pub id: u32,
    pub name: &'static str,
    pub on: DomainOn,
}

/// How a power domain comes up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainOn {
    /// The SoC keeps it on: nothing to write.
    Always,
    /// Hardware-sequenced: `mode` in `ctrl` hands the domain to the power sequencer, a rising
    /// `request` asks the sequencer to power it up, and `on` in `status` reports it up.
    /// Offsets in the provider's window.
    Sequenced { ctrl: u16, mode: u32, request: u32, status: u16, on: u32 },
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

/// Look a power domain up by provider and id.
pub fn domain(provider: ProviderKind, id: u32) -> Option<&'static DomainEntry> {
    k1::DOMAINS.iter().find(|d| d.provider == provider && d.id == id)
}

/// The register offset of pad `pin` inside a pad controller's window.
pub fn pad_offset(provider: ProviderKind, pin: u32) -> Option<u16> {
    match provider {
        ProviderKind::Pinctrl => k1::pad_offset(pin),
        _ => None,
    }
}
