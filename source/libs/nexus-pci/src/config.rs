// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: configuration space: the registers the planner reads and writes, and ECAM —
//! each function's 4 KiB of configuration at `bus << 20 | device << 15 | function << 12`
//! from the window's first bus — over `nexus_hal::Bus` in aligned 32-bit words.
//! OWNERS: @runtime @drivers

use nexus_hal::Bus;

use crate::Bdf;

/// Vendor [15:0], device [31:16] (all ones: nothing answers).
pub const ID: u16 = 0x00;
/// Command [15:0], status [31:16] (status bits clear when written with 1: write the
/// command alone).
pub const COMMAND: u16 = 0x04;
/// Revision [7:0], programming interface [15:8], subclass [23:16], base class [31:24].
pub const CLASS: u16 = 0x08;
/// Header type [22:16] and multi-function [23].
pub const HEADER: u16 = 0x0C;
/// The first base address register.
pub const BAR0: u16 = 0x10;
/// Interrupt line [7:0], interrupt pin [15:8] (1..=4: INTA..INTD).
pub const INTERRUPT: u16 = 0x3C;

/// Command: I/O space decoding.
pub const CMD_IO: u32 = 1 << 0;
/// Command: memory space decoding.
pub const CMD_MEMORY: u32 = 1 << 1;
/// Command: the function may master the bus (DMA).
pub const CMD_MASTER: u32 = 1 << 2;

/// Configuration space bytes of one bus.
pub const BUS_BYTES: u64 = 1 << 20;

/// Word access to functions' configuration registers.
pub trait ConfigSpace {
    /// The aligned word at `reg` of `bdf`; all ones when nothing answers.
    fn read(&self, bdf: Bdf, reg: u16) -> u32;
    /// Write the aligned word at `reg` of `bdf` (ignored where nothing answers).
    fn write(&self, bdf: Bdf, reg: u16, value: u32);
}

/// ECAM through a mapped window whose first byte is bus `first` (`buses` buses long).
pub struct Ecam<B: Bus> {
    bus: B,
    base: usize,
    first: u8,
    buses: u16,
}

impl<B: Bus> Ecam<B> {
    /// The window at `base` (as `bus` addresses it) holding buses `first..first + buses`.
    pub fn new(bus: B, base: usize, first: u8, buses: u16) -> Self {
        Self { bus, base, first, buses }
    }

    fn offset(&self, bdf: Bdf, reg: u16) -> Option<usize> {
        let index = u16::from(bdf.bus.checked_sub(self.first)?);
        let inside = index < self.buses && bdf.dev < 32 && bdf.func < 8 && reg < 4096;
        let aligned = reg % 4 == 0;
        (inside && aligned).then(|| {
            self.base
                + ((usize::from(index) << 20)
                    | (usize::from(bdf.dev) << 15)
                    | (usize::from(bdf.func) << 12)
                    | usize::from(reg))
        })
    }
}

impl<B: Bus> ConfigSpace for Ecam<B> {
    fn read(&self, bdf: Bdf, reg: u16) -> u32 {
        self.offset(bdf, reg).map_or(u32::MAX, |at| self.bus.read(at))
    }

    fn write(&self, bdf: Bdf, reg: u16, value: u32) {
        if let Some(at) = self.offset(bdf, reg) {
            self.bus.write(at, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Log(RefCell<Vec<(usize, Option<u32>)>>);

    impl Bus for &Log {
        fn read(&self, addr: usize) -> u32 {
            self.0.borrow_mut().push((addr, None));
            0x1234_5678
        }
        fn write(&self, addr: usize, value: u32) {
            self.0.borrow_mut().push((addr, Some(value)));
        }
    }

    const BASE: usize = 0x5000_0000;

    fn bdf(bus: u8, dev: u8, func: u8) -> Bdf {
        Bdf { bus, dev, func }
    }

    #[test]
    fn ecam_addresses_bus_device_function_and_register() {
        let log = Log::default();
        let ecam = Ecam::new(&log, BASE, 0, 256);
        assert_eq!(ecam.read(bdf(0, 1, 0), BAR0), 0x1234_5678);
        ecam.write(bdf(2, 31, 7), 0xFFC, 7);
        assert_eq!(
            *log.0.borrow(),
            [
                (BASE + (1 << 15) + 0x10, None),
                (BASE + (2 << 20) + (31 << 15) + (7 << 12) + 0xFFC, Some(7))
            ]
        );
    }

    #[test]
    fn a_window_that_starts_at_a_later_bus_counts_from_it() {
        let log = Log::default();
        let ecam = Ecam::new(&log, BASE, 4, 1);
        ecam.read(bdf(4, 0, 0), ID);
        assert_eq!(*log.0.borrow(), [(BASE, None)]);
    }

    #[test]
    fn test_reject_config_accesses_outside_the_window() {
        let log = Log::default();
        let ecam = Ecam::new(&log, BASE, 4, 1);
        for (b, reg) in [
            (bdf(3, 0, 0), ID),
            (bdf(5, 0, 0), ID),
            (bdf(4, 32, 0), ID),
            (bdf(4, 0, 8), ID),
            (bdf(4, 0, 0), 0x1002),
            (bdf(4, 0, 0), 4096),
        ] {
            assert_eq!(ecam.read(b, reg), u32::MAX, "{b:?} {reg:#x}");
            ecam.write(b, reg, 1);
        }
        assert!(log.0.borrow().is_empty(), "nothing outside the window is touched");
    }
}
