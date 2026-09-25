// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The controller: what it can do (version, capabilities, base clock), and how it is set
//! up — reset, power, the card clock, the bus width, the timing, 1.8 V signalling and the
//! enhanced strobe. The SoC layer ([`Layer`]) adds its steps at the points the upstream
//! driver documents (after a full reset, before a clock change, at a timing change, for the
//! strobe). The command engine is `engine.rs`.
//! OWNERS: @runtime @drivers

use nexus_hal::Bus;

use crate::regs::*;
use crate::{clock, k1, Error, Platform, Stage};

/// A full or line reset clears within this.
pub const RESET_TIMEOUT_US: u64 = 100_000;
/// The internal clock (and its PLL) is stable within this.
pub const CLOCK_STABLE_TIMEOUT_US: u64 = 150_000;
/// Between two reads of a state that raises no interrupt.
pub const POLL_US: u64 = 10;
/// 1.8 V signalling settles within this.
pub const SIGNAL_SETTLE_US: u64 = 5_000;

/// The SoC layer on top of the standard core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// A standard controller (QEMU's `sdhci-pci`).
    Standard,
    /// The K1's eMMC host (`spacemit,k1-sdhci` without `no-mmc`): the vendor PHY registers,
    /// HS400 enhanced strobe with the DLL.
    K1,
}

/// A bus timing the host drives; the card is switched to match before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timing {
    /// Legacy: up to 26 MHz, SDR.
    Legacy,
    /// High speed: up to 52 MHz, SDR.
    Hs,
    /// HS400: up to 200 MHz, DDR, data strobe.
    Hs400,
}

/// What the board says about this host (its tree node; TASK-0246 P4 fills it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostConfig {
    /// The base clock when the capability register names none or cannot be trusted (the
    /// K1: the `io` clock's rate).
    pub base_clock_hz: Option<u32>,
    /// The widest data bus the board wires (`bus-width`: 1, 4 or 8).
    pub bus_width: u8,
    /// The board allows HS400 enhanced strobe (`mmc-hs400-1_8v`,
    /// `mmc-hs400-enhanced-strobe`).
    pub hs400es: bool,
    /// The SoC layer.
    pub layer: Layer,
}

/// An SDHCI controller with one slot.
pub struct Host<B: Bus, P: Platform> {
    pub(crate) bus: B,
    pub(crate) platform: P,
    config: HostConfig,
    version: u8,
    pub(crate) caps: u32,
    base_hz: u32,
    clock_hz: u32,
    timing: Timing,
    width: u8,
}

/// Poll `done` every `every_us` until it holds or `limit_us` passed — for controller states
/// that raise no interrupt. `done` is asked once more after the deadline.
pub(crate) fn poll<B: Bus, P: Platform>(
    bus: &B,
    platform: &mut P,
    stage: Stage,
    limit_us: u64,
    every_us: u64,
    done: impl Fn(&B) -> bool,
) -> Result<(), Error> {
    let deadline = platform.now_us().saturating_add(limit_us);
    loop {
        if done(bus) {
            return Ok(());
        }
        if platform.now_us() >= deadline {
            return Err(Error::Timeout(stage));
        }
        platform.delay_us(every_us);
    }
}

/// Read-modify-write the bits `clear` then `set` of the word at `off`.
pub(crate) fn rmw<B: Bus>(bus: &B, off: usize, clear: u32, set: u32) {
    bus.write(off, (bus.read(off) & !clear) | set);
}

impl<B: Bus, P: Platform> Host<B, P> {
    /// The controller behind `bus` as `config` describes it.
    pub fn new(bus: B, platform: P, config: HostConfig) -> Result<Self, Error> {
        if !matches!(config.bus_width, 1 | 4 | 8) {
            return Err(Error::Config);
        }
        let version = ((bus.read(VERSION) >> 16) & 0xFF) as u8;
        let caps = bus.read(CAPS);
        let field = if version >= SPEC_300 { 0xFF } else { 0x3F };
        let named = ((caps >> CAP_BASE_CLOCK_SHIFT) & field) * 1_000_000;
        let base_hz = config.base_clock_hz.unwrap_or(named);
        if base_hz == 0 {
            return Err(Error::NoBaseClock);
        }
        Ok(Self {
            bus,
            platform,
            config,
            version,
            caps,
            base_hz,
            clock_hz: 0,
            timing: Timing::Legacy,
            width: 1,
        })
    }

    /// The specification version (`VERSION[23:16]`: 2 = 3.00).
    pub fn version(&self) -> u8 {
        self.version
    }

    /// The base clock in Hz.
    pub fn base_hz(&self) -> u32 {
        self.base_hz
    }

    /// The card clock in Hz (0 while stopped).
    pub fn clock_hz(&self) -> u32 {
        self.clock_hz
    }

    /// The timing the host drives.
    pub fn timing(&self) -> Timing {
        self.timing
    }

    /// The data bus width.
    pub fn width(&self) -> u8 {
        self.width
    }

    /// The SoC layer.
    pub fn layer(&self) -> Layer {
        self.config.layer
    }

    /// The widest bus the controller and the board share.
    pub fn max_width(&self) -> u8 {
        if self.config.bus_width == 8 && self.caps & CAP_8_BIT != 0 {
            8
        } else if self.config.bus_width >= 4 {
            4
        } else {
            1
        }
    }

    /// True when this host may run HS400 enhanced strobe (the layer has it, the board
    /// allows it, an 8-bit bus exists).
    pub fn can_hs400es(&self) -> bool {
        self.config.layer == Layer::K1 && self.config.hs400es && self.max_width() == 8
    }

    /// The platform (for a caller's own waits).
    pub fn platform_mut(&mut self) -> &mut P {
        &mut self.platform
    }

    /// Full reset, then the layer's setup, the interrupt masks, the longest data timeout and
    /// bus power at the highest voltage the controller names: the controller as after
    /// power-up, the card clock stopped, 1-bit legacy.
    pub fn reset(&mut self) -> Result<(), Error> {
        self.bus.write(CLOCK, RESET_ALL);
        poll(&self.bus, &mut self.platform, Stage::Reset, RESET_TIMEOUT_US, POLL_US, |b| {
            b.read(CLOCK) & RESET_ALL == 0
        })?;
        if self.config.layer == Layer::K1 {
            k1::after_reset(&self.bus);
        }
        self.bus.write(INT_STATUS, INT_USED);
        self.bus.write(INT_ENABLE, INT_USED);
        self.bus.write(INT_SIGNAL, if self.platform.irq() { INT_USED } else { 0 });
        self.bus.write(CLOCK, TIMEOUT_MAX);
        let vdd = if self.caps & CAP_330 != 0 {
            PWR_330
        } else if self.caps & CAP_300 != 0 {
            PWR_300
        } else if self.caps & CAP_180 != 0 {
            PWR_180
        } else {
            return Err(Error::Unsupported);
        };
        rmw(&self.bus, HOST_CTRL, PWR_VDD_MASK | PWR_ON, vdd);
        rmw(&self.bus, HOST_CTRL, 0, PWR_ON);
        (self.clock_hz, self.timing, self.width) = (0, Timing::Legacy, 1);
        Ok(())
    }

    /// Run the card clock at the fastest rate not above `target_hz`; returns the rate.
    pub fn set_clock(&mut self, target_hz: u32) -> Result<u32, Error> {
        if self.config.layer == Layer::K1 {
            k1::before_clock(&self.bus, self.timing);
        }
        let (bits, actual) =
            clock::divider(self.version, self.base_hz, target_hz).ok_or(Error::Clock)?;
        let keep = self.bus.read(CLOCK) & TIMEOUT_MASK;
        self.bus.write(CLOCK, keep);
        self.clock_hz = 0;
        let mut on = keep | bits | CLK_INT_EN;
        self.bus.write(CLOCK, on);
        self.clock_stable()?;
        if self.version >= SPEC_410 {
            on |= CLK_PLL_EN;
            self.bus.write(CLOCK, on);
            self.clock_stable()?;
        }
        self.bus.write(CLOCK, on | CLK_CARD_EN);
        self.clock_hz = actual;
        Ok(actual)
    }

    fn clock_stable(&mut self) -> Result<(), Error> {
        let limit = CLOCK_STABLE_TIMEOUT_US;
        poll(&self.bus, &mut self.platform, Stage::ClockStable, limit, POLL_US, |b| {
            b.read(CLOCK) & CLK_INT_STABLE != 0
        })
    }

    /// Drive a 1-, 4- or 8-bit data bus (the card was switched first).
    pub fn set_width(&mut self, width: u8) -> Result<(), Error> {
        let bits = match width {
            1 => 0,
            4 => HC1_WIDTH_4,
            8 => HC1_WIDTH_8,
            _ => return Err(Error::Config),
        };
        rmw(&self.bus, HOST_CTRL, HC1_WIDTH_4 | HC1_WIDTH_8, bits);
        self.width = width;
        Ok(())
    }

    /// Change the timing with the card clock stopped (SDHCI 3.00 §3.2.3), then run the clock
    /// at the fastest rate not above `target_hz`; returns the rate.
    pub fn set_timing(&mut self, timing: Timing, target_hz: u32) -> Result<u32, Error> {
        rmw(&self.bus, CLOCK, RESET_MASK | CLK_CARD_EN, 0);
        let hs = if timing == Timing::Legacy { 0 } else { HC1_HIGH_SPEED };
        rmw(&self.bus, HOST_CTRL, HC1_HIGH_SPEED, hs);
        let uhs = if timing == Timing::Hs400 { HC2_UHS_HS400 } else { 0 };
        rmw(&self.bus, HOST_CTRL2, HC2_UHS_MASK, uhs);
        if self.config.layer == Layer::K1 {
            k1::set_timing(&self.bus, timing);
        }
        self.timing = timing;
        self.set_clock(target_hz)
    }

    /// 1.8 V signalling (before HS400); refused when the bit does not stay set.
    pub fn signal_1v8(&mut self) -> Result<(), Error> {
        rmw(&self.bus, HOST_CTRL2, 0, HC2_1V8);
        self.platform.delay_us(SIGNAL_SETTLE_US);
        if self.bus.read(HOST_CTRL2) & HC2_1V8 == 0 {
            return Err(Error::Signal1v8);
        }
        Ok(())
    }

    /// The enhanced strobe for HS400 (the K1: strobe enable, the DLL locked).
    pub fn enhanced_strobe(&mut self) -> Result<(), Error> {
        match self.config.layer {
            Layer::K1 => k1::enhanced_strobe(&self.bus, &mut self.platform),
            Layer::Standard => Err(Error::Unsupported),
        }
    }

    /// Reset the command and/or data line (`RESET_CMD`, `RESET_DATA`) after an error.
    pub(crate) fn reset_lines(&mut self, lines: u32) -> Result<(), Error> {
        rmw(&self.bus, CLOCK, RESET_MASK, lines);
        poll(&self.bus, &mut self.platform, Stage::Reset, RESET_TIMEOUT_US, POLL_US, |b| {
            b.read(CLOCK) & lines == 0
        })
    }
}
