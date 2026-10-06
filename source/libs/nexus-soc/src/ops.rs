// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The executor: steps over a `Bus`, every write read back, every self-clearing
//! bit and every power domain's status polled with a bound. A step already
//! satisfied writes nothing — the stock state of the board is the first proof
//! (`tests/k1.rs`).

use nexus_hal::Bus;

use crate::field::Field;
use crate::plan::{Plan, Step};
use crate::table::{k1, ClockEntry};

/// What holds the executor for a settle step (a supply's start-up delay): `socd`'s kernel
/// one-shot; a test's recorder. The executor has no clock of its own.
pub trait Pause {
    /// Return after `ms` milliseconds.
    fn pause_ms(&self, ms: u32);
}

/// Bound on polling a self-clearing bit (bus reads, not time: the executor has
/// no clock; `socd` bounds the wall-clock around it).
pub const FC_POLL_READS: usize = 1000;

/// Bound on polling a power domain's status (bus reads): a power-up outlasts a clock switch
/// (the rail ramps before the sequencer lifts isolation); at a few hundred nanoseconds per
/// APMU read this caps the wait at tens of milliseconds.
pub const DOMAIN_POLL_READS: usize = 100_000;

/// Bound on reading a GPIO bank's level word after a set/clear (bus reads): the word reports
/// the pin, which follows the driver after the pad's propagation.
pub const GPIO_LEVEL_POLL_READS: usize = 1000;

/// Why a step failed: the register and what it read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A write did not read back (`addr`, the value read).
    ReadBack { addr: usize, value: u32 },
    /// The frequency-change bit never cleared.
    FcStuck { addr: usize, value: u32 },
    /// A power domain never reported on (`addr` = its status register, the word read last).
    DomainStuck { addr: usize, value: u32 },
    /// A mux/div value outside the field.
    Range,
    /// The plan holds a settle and the executor was given nothing to pause with.
    NoPause,
}

impl Fault {
    /// The step kind that failed, for markers: `read-back`, `frequency-change`, `domain`,
    /// `range`, `settle`.
    pub fn step(&self) -> &'static str {
        match self {
            Fault::ReadBack { .. } => "read-back",
            Fault::FcStuck { .. } => "frequency-change",
            Fault::DomainStuck { .. } => "domain",
            Fault::Range => "range",
            Fault::NoPause => "settle",
        }
    }

    /// The register and the word it read, when the fault has one.
    pub fn register(&self) -> Option<(usize, u32)> {
        match *self {
            Fault::ReadBack { addr, value }
            | Fault::FcStuck { addr, value }
            | Fault::DomainStuck { addr, value } => Some((addr, value)),
            Fault::Range | Fault::NoPause => None,
        }
    }
}

/// What the executor did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub steps: usize,
    pub writes: usize,
    pub resets_released: usize,
    pub clocks_on: usize,
    pub rates_set: usize,
    pub domains: usize,
    pub pads: usize,
    /// GPIO lines driven (TASK-0328 U3).
    pub gpios: usize,
    /// Whole words written as the binding names them (`nexus,glue-words`).
    pub words: usize,
    /// Milliseconds held in settles.
    pub settled_ms: u32,
}

pub struct Executor<'b, B: Bus> {
    bus: &'b B,
    pause: Option<&'b dyn Pause>,
}

impl<'b, B: Bus> Executor<'b, B> {
    /// An executor without a clock: a plan with a settle faults (`NoPause`).
    pub fn new(bus: &'b B) -> Self {
        Executor { bus, pause: None }
    }

    /// An executor that can hold for a settle.
    pub fn with_pause(bus: &'b B, pause: &'b dyn Pause) -> Self {
        Executor { bus, pause: Some(pause) }
    }

    /// Run a plan in order; stop at the first fault.
    pub fn execute(&self, plan: &Plan) -> Result<Report, Fault> {
        let mut report = Report::default();
        for step in plan.steps() {
            report.steps += 1;
            match *step {
                Step::SetWord { addr, mask, value } => {
                    if self.set_bits(addr, mask, value)? {
                        report.writes += 1;
                    }
                    report.words += 1;
                }
                Step::GpioOut { bank, bit, high } => {
                    report.writes += self.gpio_out(bank, bit, high)?;
                    report.gpios += 1;
                }
                Step::Settle { ms } => {
                    let Some(pause) = self.pause else { return Err(Fault::NoPause) };
                    pause.pause_ms(ms);
                    report.settled_ms = report.settled_ms.saturating_add(ms);
                }
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
                Step::SetRate { clock, window, mux, div } => {
                    report.writes += self.set_rate(clock, window, mux, div)?;
                    report.rates_set += 1;
                }
                Step::DomainAssumedOn { .. } => report.domains += 1,
                Step::DomainOn { ctrl, mode, request, status, on, .. } => {
                    report.writes += self.domain_on(ctrl, mode, request, status, on)?;
                    report.domains += 1;
                }
                Step::PadSet { addr, mask, value } => {
                    if self.set_bits(addr, mask, value)? {
                        report.writes += 1;
                    }
                    report.pads += 1;
                }
            }
        }
        Ok(report)
    }

    /// Drive one line of the bank at `bank` as an output at `high`; returns the writes it took.
    /// The direction and the level are set through the bank's masked set/clear registers (no
    /// read-modify-write: another writer's lines are never touched) and read back from its
    /// direction and level words; a line already there is left alone.
    fn gpio_out(&self, bank: usize, bit: u32, high: bool) -> Result<usize, Fault> {
        let mut writes = 0;
        let direction = bank + k1::GPIO_DIRECTION as usize;
        if self.bus.read(direction) & bit == 0 {
            self.bus.write(bank + k1::GPIO_SET_DIRECTION as usize, bit);
            let back = self.bus.read(direction);
            if back & bit == 0 {
                return Err(Fault::ReadBack { addr: direction, value: back });
            }
            writes += 1;
        }
        let level = bank + k1::GPIO_LEVEL as usize;
        let want = if high { bit } else { 0 };
        if self.bus.read(level) & bit != want {
            let reg = if high { k1::GPIO_SET } else { k1::GPIO_CLEAR };
            self.bus.write(bank + reg as usize, bit);
            // The level word follows the pin, not the write: board cycle 5 (2026-10-05) read the
            // old level right after the set and the new one a moment later — a bounded number
            // of reads, never one.
            let mut back = self.bus.read(level);
            for _ in 0..GPIO_LEVEL_POLL_READS {
                if back & bit == want {
                    break;
                }
                back = self.bus.read(level);
            }
            if back & bit != want {
                return Err(Fault::ReadBack { addr: level, value: back });
            }
            writes += 1;
        }
        Ok(writes)
    }

    /// Bring a hardware-sequenced domain up; returns the writes it took. A domain that
    /// already reports on is left alone. Else: the mode bit with the request low, then the
    /// request raised — a rising request whatever state the control word was left in — and
    /// the status polled with a bound.
    fn domain_on(
        &self,
        ctrl: usize,
        mode: u32,
        request: u32,
        status: usize,
        on: u32,
    ) -> Result<usize, Fault> {
        if self.bus.read(status) & on == on {
            return Ok(0);
        }
        let mut writes = 0;
        if self.set_bits(ctrl, mode | request, mode)? {
            writes += 1;
        }
        if self.set_bits(ctrl, request, request)? {
            writes += 1;
        }
        for _ in 0..DOMAIN_POLL_READS {
            if self.bus.read(status) & on == on {
                return Ok(writes);
            }
        }
        Err(Fault::DomainStuck { addr: status, value: self.bus.read(status) })
    }

    /// Make a clock run at the selected parent and divider; returns the writes it took. A
    /// clock already there is left alone (no frequency change triggered).
    fn set_rate(
        &self,
        clock: &ClockEntry,
        window: usize,
        mux: u32,
        div: u32,
    ) -> Result<usize, Fault> {
        let (Some(mux_f), Some(div_f)) = (clock.mux, clock.div) else { return Err(Fault::Range) };
        let word = self.bus.read(window + clock.reg as usize);
        if mux_f.get(word) == mux && div_f.get(word) == div {
            return Ok(0);
        }
        self.set_mux_div(clock, window, mux, div)?;
        Ok(if clock.fc != 0 { 2 } else { 1 })
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
