// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The executor: steps over a `Bus`, every write read back, every self-clearing
//! bit polled with a bound. A step already satisfied writes nothing — the
//! stock state of the board is the first proof (`tests/k1.rs`).

use nexus_hal::Bus;

use crate::field::Field;
use crate::plan::{Plan, Step};
use crate::table::ClockEntry;

/// Bound on polling a self-clearing bit (bus reads, not time: the executor has
/// no clock; `socd` bounds the wall-clock around it).
pub const FC_POLL_READS: usize = 1000;

/// Why a step failed: the register and what it read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A write did not read back (`addr`, the value read).
    ReadBack { addr: usize, value: u32 },
    /// The frequency-change bit never cleared.
    FcStuck { addr: usize, value: u32 },
    /// A mux/div value outside the field.
    Range,
}

/// What the executor did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub steps: usize,
    pub writes: usize,
    pub resets_released: usize,
    pub clocks_on: usize,
    pub domains: usize,
}

pub struct Executor<'b, B: Bus> {
    bus: &'b B,
}

impl<'b, B: Bus> Executor<'b, B> {
    pub fn new(bus: &'b B) -> Self {
        Executor { bus }
    }

    /// Run a plan in order; stop at the first fault.
    pub fn execute(&self, plan: &Plan) -> Result<Report, Fault> {
        let mut report = Report::default();
        for step in plan.steps() {
            report.steps += 1;
            match *step {
                Step::ReleaseReset { addr, mask, assert_sets } => {
                    let want = if assert_sets { 0 } else { mask };
                    if self.set_bits(addr, mask, want)? {
                        report.writes += 1;
                    }
                    report.resets_released += 1;
                }
                Step::GateOn { addr, mask } => {
                    if self.set_bits(addr, mask, mask)? {
                        report.writes += 1;
                    }
                    report.clocks_on += 1;
                }
                Step::DomainAssumedOn { .. } => report.domains += 1,
            }
        }
        Ok(report)
    }

    /// Make `mask` of the register at `addr` read `want`; true when a write was
    /// needed. Read-modify-write with read-back.
    pub fn set_bits(&self, addr: usize, mask: u32, want: u32) -> Result<bool, Fault> {
        let cur = self.bus.read(addr);
        if cur & mask == want & mask {
            return Ok(false);
        }
        let next = (cur & !mask) | (want & mask);
        self.bus.write(addr, next);
        let back = self.bus.read(addr);
        if back & mask != want & mask {
            return Err(Fault::ReadBack { addr, value: back });
        }
        Ok(true)
    }

    /// Select `mux`/`div` of a clock and trigger the frequency change: write the
    /// fields, set the FC bit, poll until the hardware clears it.
    pub fn set_mux_div(
        &self,
        entry: &ClockEntry,
        window: usize,
        mux: u32,
        div: u32,
    ) -> Result<(), Fault> {
        let (Some(mux_f), Some(div_f)) = (entry.mux, entry.div) else { return Err(Fault::Range) };
        if mux >= (1 << mux_f.width) || div >= (1 << div_f.width) {
            return Err(Fault::Range);
        }
        let addr = window + entry.reg as usize;
        let cur = self.bus.read(addr);
        let next = div_f.set(mux_f.set(cur, mux), div);
        if next != cur {
            self.bus.write(addr, next);
            let back = self.bus.read(addr);
            if back & (mux_f.mask() | div_f.mask()) != next & (mux_f.mask() | div_f.mask()) {
                return Err(Fault::ReadBack { addr, value: back });
            }
        }
        if entry.fc != 0 {
            let fc_addr = window + entry.fc_reg as usize;
            self.bus.write(fc_addr, self.bus.read(fc_addr) | entry.fc);
            for _ in 0..FC_POLL_READS {
                let v = self.bus.read(fc_addr);
                if v & entry.fc == 0 {
                    return Ok(());
                }
            }
            let value = self.bus.read(fc_addr);
            return Err(Fault::FcStuck { addr: fc_addr, value });
        }
        Ok(())
    }

    /// The clock's rate as the registers say: the selected parent divided by
    /// `div + 1`, or the fixed parent of a gate-only clock (0 = unknown).
    pub fn rate(&self, entry: &ClockEntry, window: usize) -> u64 {
        let word = self.bus.read(window + entry.reg as usize);
        match (entry.mux, entry.div) {
            (Some(mux), div) if !entry.parents.is_empty() => {
                let parent = entry.parents.get(mux.get(word) as usize).map_or(0, |p| p.hz);
                let d = div.map_or(0, |f: Field| f.get(word)) as u64 + 1;
                parent / d
            }
            _ => entry.fixed_parent_hz,
        }
    }

    /// Is the clock's gate on (or the clock ungated)?
    pub fn is_on(&self, entry: &ClockEntry, window: usize) -> bool {
        entry.gate == 0 || self.bus.read(window + entry.reg as usize) & entry.gate == entry.gate
    }
}
