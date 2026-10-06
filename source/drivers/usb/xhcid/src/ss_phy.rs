// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The SuperSpeed (combo) PHY of the host's USB 3.0 root port (TASK-0328 U3, RFC-0099 §6):
//! the xHCI inside the DWC3 waits on this PHY's PIPE clock for its reset (board cycles 4-9:
//! HCRST never completed with the block reading all zeros — the PHY held in PCIe port A's
//! global reset, which socd releases from the tree before xhcid runs). [`compare`] reads the
//! block's words against the stock system's (2026-10-04, `docs/board/measurements/
//! 2026-10-04-usb-topology/stock-usb-regs.txt`, the first lane's non-zero words); [`init`] runs
//! the PHY's USB-mode PLL sequence as the vendor PHY driver does it (reference only, RFC-0099:
//! the test word zeroed, the internal timer set for USB, the 24 MHz reference and the
//! spread-spectrum depth, the software init-done bit, then the PLL's lock polled, bounded),
//! every write read back. The stock words agree with that sequence (`0x08 = 0x97d`: timer 2,
//! init done, locked; `0x48 = 0x603a2276`: 5000 ppm, 24 MHz reference).

use nexus_hal::Bus;

use crate::phy::{compare_table, Phy};

/// The stock words (`phy@c0b10000`, the first lane's block; the dump repeats at
/// +0x100/+0x200/+0x300).
pub const SS_STOCK: [(usize, u32); 23] = [
    (0x00, 0x2020_0514),
    (0x08, 0x0000_097d),
    (0x0c, 0x0f55_698c),
    (0x10, 0x0000_000c),
    (0x1c, 0x0000_8000),
    (0x20, 0x2808_1087),
    (0x40, 0xa400_0101),
    (0x48, 0x603a_2276),
    (0x4c, 0x5512_8d70),
    (0x50, 0x0048_788c),
    (0x54, 0x7090_0bb5),
    (0x58, 0x8842_8b00),
    (0x5c, 0x0000_0508),
    (0x60, 0x0000_0120),
    (0x64, 0x0c50_8900),
    (0x80, 0x0000_0400),
    (0x84, 0x0000_6d37),
    (0x88, 0x000f_ff50),
    (0x90, 0x0000_7000),
    (0x94, 0x4a4a_0042),
    (0x98, 0x4a4a_4a4a),
    (0x9c, 0x4a4a_4a4a),
    (0xa0, 0x20f7_f7bc),
];

/// The PLL configuration word: bit 0 reports the PLL locked, bits 10:7 the internal timer
/// (2 = USB), bit 11 the software's init-done.
pub const PU_ADDR_CLK_CFG: usize = 0x08;
const PLL_READY: u32 = 1;
const TIMER_ADJ_MASK: u32 = 0xf << 7;
const TIMER_ADJ_USB: u32 = 0x2 << 7;
const SW_PHY_INIT_DONE: u32 = 1 << 11;
/// The PLL's reference word: bit 12 a 100 MHz reference with spread spectrum (clear), bits
/// 15:13 the reference select (1 = 24 MHz), bits 19:16 the spread-spectrum depth (0xa = 5000
/// ppm for USB).
pub const PU_PLL_1: usize = 0x48;
const REF_100_WSSC: u32 = 1 << 12;
const FREF_SEL_MASK: u32 = 0x7 << 13;
const FREF_24M: u32 = 0x1 << 13;
const SSC_DEP_MASK: u32 = 0xf << 16;
const SSC_DEP_5000PPM: u32 = 0xa << 16;
/// The USB 3.0 test control word: zero in USB mode.
pub const USB3_TEST_CTRL: usize = 0x68;
/// The calibration result: bit 10 reports the resistor tune done (the PHY calibrated — by the
/// stock PHY driver on PCIe port A's application clocks; whether our loader leaves it done is
/// cycle 10's measurement).
pub const RCAL_RESULT: usize = 0x84;
const R_TUNE_DONE: u32 = 1 << 10;
/// The PLL's lock, polled on the one-shot: 100 × 5 ms (the vendor driver waits 500 ms).
pub const PLL_LOCK_POLL_MS: u32 = 5;
pub const PLL_LOCK_READS: u32 = 100;

/// Read the PHY's words and compare them with the stock system's.
pub fn compare<B: Bus>(bus: &B) -> Phy {
    compare_table(bus, SS_STOCK.iter().copied())
}

/// What [`init`] found and did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SsInit {
    /// The calibration was done before the sequence ran.
    pub calibrated: bool,
    /// Words written (0: the PHY was configured already).
    pub writes: u8,
    /// The PLL reported locked within the bound.
    pub pll_ready: bool,
    /// How long the lock was waited for.
    pub waited_ms: u32,
    /// The PLL configuration word as left.
    pub cfg: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SsError {
    /// A write did not read back (the register and the word read): the block is dead or held.
    ReadBack(usize, u32),
}

/// Run the USB-mode PLL sequence; `pause` is the caller's bounded wait (a kernel one-shot,
/// never a spin). A PLL that never locks is reported, not an error: the controller's reset
/// that follows measures it.
pub fn init<B: Bus>(bus: &B, mut pause: impl FnMut(u32)) -> Result<SsInit, SsError> {
    let calibrated = bus.read(RCAL_RESULT) & R_TUNE_DONE != 0;
    let mut writes = 0u8;
    set(bus, USB3_TEST_CTRL, u32::MAX, 0, &mut writes)?;
    set(bus, PU_ADDR_CLK_CFG, TIMER_ADJ_MASK, TIMER_ADJ_USB, &mut writes)?;
    set(
        bus,
        PU_PLL_1,
        SSC_DEP_MASK | REF_100_WSSC | FREF_SEL_MASK,
        SSC_DEP_5000PPM | FREF_24M,
        &mut writes,
    )?;
    set(bus, PU_ADDR_CLK_CFG, SW_PHY_INIT_DONE, SW_PHY_INIT_DONE, &mut writes)?;
    let mut cfg = bus.read(PU_ADDR_CLK_CFG);
    let mut waited_ms = 0;
    let mut reads = 0;
    while cfg & PLL_READY == 0 && reads < PLL_LOCK_READS {
        pause(PLL_LOCK_POLL_MS);
        waited_ms += PLL_LOCK_POLL_MS;
        reads += 1;
        cfg = bus.read(PU_ADDR_CLK_CFG);
    }
    Ok(SsInit { calibrated, writes, pll_ready: cfg & PLL_READY != 0, waited_ms, cfg })
}

/// Make `mask` of the word at `reg` read `want`, read back; a word already so is not written.
fn set<B: Bus>(bus: &B, reg: usize, mask: u32, want: u32, writes: &mut u8) -> Result<(), SsError> {
    let before = bus.read(reg);
    if before & mask == want {
        return Ok(());
    }
    bus.write(reg, (before & !mask) | want);
    let after = bus.read(reg);
    if after & mask != want {
        return Err(SsError::ReadBack(reg, after));
    }
    *writes += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    /// A register file whose PLL reports locked `locks_after` polls of the configuration word
    /// once the init-done bit is set (0: never).
    struct Regs {
        words: RefCell<HashMap<usize, u32>>,
        writes: RefCell<Vec<(usize, u32)>>,
        polls: Cell<u32>,
        locks_after: u32,
    }

    impl Regs {
        fn with(words: &[(usize, u32)], locks_after: u32) -> Self {
            Regs {
                words: RefCell::new(words.iter().copied().collect()),
                writes: RefCell::new(Vec::new()),
                polls: Cell::new(0),
                locks_after,
            }
        }
    }

    impl Bus for Regs {
        fn read(&self, addr: usize) -> u32 {
            let word = self.words.borrow().get(&addr).copied().unwrap_or(0);
            if addr == PU_ADDR_CLK_CFG && word & SW_PHY_INIT_DONE != 0 && self.locks_after != 0 {
                let polls = self.polls.get() + 1;
                self.polls.set(polls);
                return word | u32::from(polls > self.locks_after);
            }
            word
        }
        fn write(&self, addr: usize, value: u32) {
            self.writes.borrow_mut().push((addr, value));
            self.words.borrow_mut().insert(addr, value);
        }
    }

    #[test]
    fn the_stock_phy_is_configured_calibrated_and_locked_and_nothing_is_written() {
        let bus = Regs::with(&SS_STOCK, 0);
        assert_eq!((compare(&bus).matches, compare(&bus).ndiffs), (23, 0));
        let stock = init(&bus, |_| panic!("no wait")).unwrap();
        assert_eq!(
            stock,
            SsInit { calibrated: true, writes: 0, pll_ready: true, waited_ms: 0, cfg: 0x97d }
        );
        assert!(bus.writes.borrow().is_empty());
    }

    /// A cold block (the loader's: all zeros once the reset is released): the timer, the
    /// reference and the init-done written in order (the test word is zero already), the lock
    /// waited for on the pause (the file locks on the fifth read of the configuration word
    /// after init-done: the read-back, the first poll, three more — three pauses), the
    /// calibration found not done.
    #[test]
    fn a_cold_phy_takes_the_sequence_and_the_lock_is_waited_for() {
        let bus = Regs::with(&[], 4);
        assert_eq!(compare(&bus).ndiffs, 23);
        let mut pauses = Vec::new();
        let cold = init(&bus, |ms| pauses.push(ms)).unwrap();
        assert_eq!(
            cold,
            SsInit { calibrated: false, writes: 3, pll_ready: true, waited_ms: 15, cfg: 0x901 }
        );
        assert_eq!(pauses, vec![5, 5, 5]);
        assert_eq!(
            *bus.writes.borrow(),
            vec![
                (PU_ADDR_CLK_CFG, TIMER_ADJ_USB),
                (PU_PLL_1, SSC_DEP_5000PPM | FREF_24M),
                (PU_ADDR_CLK_CFG, TIMER_ADJ_USB | SW_PHY_INIT_DONE),
            ]
        );
        // The reference word as the loader might leave it (100 MHz with spread spectrum, PCIe
        // timer): the owned fields rewritten, the rest kept.
        let pcie =
            Regs::with(&[(PU_PLL_1, 0x6000_1000 | 0x6 << 13), (PU_ADDR_CLK_CFG, 0x6 << 7)], 1);
        let relanded = init(&pcie, |_| {}).unwrap();
        assert_eq!(relanded.writes, 3);
        assert_eq!(
            pcie.read(PU_PLL_1),
            0x600a_2000,
            "depth 0xa, 24 MHz, no 100 MHz, bit 29-30 kept"
        );
    }

    /// A PLL that never locks is reported after the bound (500 ms), never an error; a word
    /// that will not take the write (the block held in reset reads zeros) stops the sequence
    /// and names itself.
    #[test]
    fn test_reject_a_pll_that_never_locks_and_a_block_that_does_not_read_back() {
        let bus = Regs::with(&[], 0);
        let mut waited = 0;
        let dead = init(&bus, |ms| waited += ms).unwrap();
        assert_eq!((dead.pll_ready, dead.waited_ms, waited), (false, 500, 500));
        struct Held;
        impl Bus for Held {
            fn read(&self, _: usize) -> u32 {
                0
            }
            fn write(&self, _: usize, _: u32) {}
        }
        assert_eq!(init(&Held, |_| {}), Err(SsError::ReadBack(PU_ADDR_CLK_CFG, 0)));
    }
}
