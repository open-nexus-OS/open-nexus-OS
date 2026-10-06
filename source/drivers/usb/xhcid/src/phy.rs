// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The USB 2.0 PHY's words (TASK-0328 U3, RFC-0099 §6): measured on the stock system with the
//! ports in use (2026-10-04, `docs/board/measurements/2026-10-04-usb-topology/
//! stock-usb-regs.txt`, the non-zero words of `usb2phy@c0a30000`). Whether our chain's loader
//! leaves the PHY in that state is a board cycle's measurement: [`compare`] reads the words and
//! says how many match and which differ (board cycle 3 read five of twelve differently), and
//! [`set_stock`] writes the differing writable words to the stock values in the order the
//! vendor PHY driver's init runs them (reference only, RFC-0099: the PLL divider word first,
//! then the PLL's lock waited for — bit 0 of the reset/mode word — then the reset/mode word,
//! the HS transmit clock word, the host-disconnect auto-clear bit), each read back, before the
//! controller is started.

use nexus_hal::Bus;

/// What a stock word is to the step: glue the PHY takes a write of; a status the PHY reports
/// (board cycle 4, 2026-10-05: `0x38` written as `0x8011` read `0x0` back — compared, never
/// written); or a word measured on the stock system whose writability no cycle has shown
/// (compared, never written — the vendor driver's init does not touch it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    Writable,
    Status,
    Compared,
}

/// The PLL divider word (`PHY_PLL_DIV_CFG`): the 24 MHz reference, the internal divider.
pub const PLL_DIV_CFG: usize = 0x98;
/// The reset/mode word (`PHY_RST_MODE_CTRL`); its bit 0 reports the PLL locked.
pub const RST_MODE_CTRL: usize = 0x04;
const PLL_READY: u32 = 1;
/// The PLL's lock, polled on the one-shot: 10 × 5 ms (the vendor driver waits 50 ms).
pub const PLL_LOCK_POLL_MS: u32 = 5;
pub const PLL_LOCK_READS: u32 = 10;

/// The stock words: (offset, value, kind), in the order the step writes them.
pub const STOCK: [(usize, u32, Word); 21] = [
    (PLL_DIV_CFG, 0xbec4, Word::Writable),
    (RST_MODE_CTRL, 0x60ef, Word::Writable),
    (0x08, 0xb028, Word::Writable),
    (0x0c, 0x0003, Word::Writable),
    (0x10, 0x0015, Word::Writable),
    (0x14, 0x3100, Word::Writable),
    (0x18, 0x0104, Word::Writable),
    (0x20, 0x71c3, Word::Writable),
    (0x2c, 0x3000, Word::Writable),
    (0x34, 0x001c, Word::Writable),
    (0x38, 0x8011, Word::Status),
    (0x48, 0x00ff, Word::Writable),
    (0x4c, 0x0101, Word::Writable),
    (0x84, 0x0300, Word::Compared),
    (0x88, 0x007f, Word::Compared),
    (0x8c, 0x4468, Word::Compared),
    (0x90, 0x4864, Word::Compared),
    (0x94, 0xec04, Word::Compared),
    (0x9c, 0xb2a5, Word::Compared),
    (0xa0, 0x0311, Word::Compared),
    (0xa4, 0x189c, Word::Compared),
];

/// The most words either PHY table compares (the SuperSpeed PHY's 23, `ss_phy.rs`).
pub const DIFFS_MAX: usize = 23;

/// How the live words compare with the stock ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phy {
    /// Words equal to the stock system's (of the table's).
    pub matches: u8,
    /// Every word that differs: its offset and the value read, in the table's order.
    pub diffs: [(usize, u32); DIFFS_MAX],
    pub ndiffs: u8,
}

impl Phy {
    /// The words that differ.
    #[must_use]
    pub fn diffs(&self) -> &[(usize, u32)] {
        &self.diffs[..usize::from(self.ndiffs)]
    }
}

/// Read the USB 2.0 PHY's words and compare them with the stock system's.
pub fn compare<B: Bus>(bus: &B) -> Phy {
    compare_table(bus, STOCK.iter().map(|(o, v, _)| (*o, *v)))
}

/// Read a table's words and compare them with the stock values (at most [`DIFFS_MAX`]).
pub(crate) fn compare_table<B: Bus>(bus: &B, table: impl Iterator<Item = (usize, u32)>) -> Phy {
    let mut phy = Phy { matches: 0, diffs: [(0, 0); DIFFS_MAX], ndiffs: 0 };
    for (offset, stock) in table.take(DIFFS_MAX) {
        let live = bus.read(offset);
        if live == stock {
            phy.matches += 1;
        } else {
            phy.diffs[usize::from(phy.ndiffs)] = (offset, live);
            phy.ndiffs += 1;
        }
    }
    phy
}

/// What [`set_stock`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Set {
    /// Words written (the ones that differed).
    pub writes: u8,
    /// The first word that did not read back as written (or the reset/mode word, when the PLL
    /// never reported locked): its offset and the value read.
    pub failed: Option<(usize, u32)>,
    /// How long the PLL's lock was waited for after the divider word.
    pub pll_wait_ms: u32,
}

/// Write every differing writable word to its stock value in the table's order, each read
/// back; after the PLL divider word the PLL's lock is waited for on `pause` (bounded), as the
/// vendor driver's init does, before the reset/mode word releases the PHY's internal resets.
pub fn set_stock<B: Bus>(bus: &B, mut pause: impl FnMut(u32)) -> Set {
    let mut set = Set { writes: 0, failed: None, pll_wait_ms: 0 };
    for (offset, stock, kind) in STOCK {
        if kind != Word::Writable || bus.read(offset) == stock {
            continue;
        }
        if offset == RST_MODE_CTRL {
            let mut mode = bus.read(RST_MODE_CTRL);
            let mut reads = 0;
            while mode & PLL_READY == 0 && reads < PLL_LOCK_READS {
                pause(PLL_LOCK_POLL_MS);
                set.pll_wait_ms += PLL_LOCK_POLL_MS;
                reads += 1;
                mode = bus.read(RST_MODE_CTRL);
            }
            if mode & PLL_READY == 0 {
                set.failed = Some((RST_MODE_CTRL, mode));
                return set;
            }
        }
        bus.write(offset, stock);
        set.writes += 1;
        let back = bus.read(offset);
        if back != stock {
            set.failed = Some((offset, back));
            return set;
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A register file that takes writes.
    struct Words(RefCell<std::collections::HashMap<usize, u32>>, RefCell<Vec<(usize, u32)>>);

    impl Words {
        fn with(f: impl Fn(usize) -> u32) -> Self {
            let regs = STOCK.iter().map(|(o, _, _)| (*o, f(*o))).collect();
            Words(RefCell::new(regs), RefCell::new(Vec::new()))
        }
    }

    impl Bus for Words {
        fn read(&self, addr: usize) -> u32 {
            self.0.borrow().get(&addr).copied().unwrap_or(0)
        }
        fn write(&self, addr: usize, value: u32) {
            self.1.borrow_mut().push((addr, value));
            self.0.borrow_mut().insert(addr, value);
        }
    }

    fn stock(off: usize) -> u32 {
        STOCK.iter().find(|(o, _, _)| *o == off).map_or(0, |(_, v, _)| *v)
    }

    #[test]
    fn the_stock_words_match_and_a_cold_phy_differs_at_every_word() {
        let phy = compare(&Words::with(stock));
        assert_eq!((phy.matches, phy.ndiffs), (21, 0));
        let cold = compare(&Words::with(|_| 0));
        assert_eq!((cold.matches, cold.ndiffs), (0, 21));
        assert_eq!(cold.diffs()[0], (PLL_DIV_CFG, 0), "the divider word leads");
        let one_off = compare(&Words::with(|off| if off == 0x20 { 0x71c2 } else { stock(off) }));
        assert_eq!((one_off.matches, one_off.diffs()), (20, &[(0x20, 0x71c2)][..]));
    }

    /// The loader's words as board cycle 4 read them (`0x04=0x60e9 0x10=0x11 0x18=0x105
    /// 0x34=0x10 0x38=0x0`, the PLL locked already): the four writable ones are written to the
    /// stock values and read back without a wait; the status word is compared, never written;
    /// the matching ones never touched.
    #[test]
    fn setting_the_stock_words_writes_only_what_differs_and_is_writable() {
        let bus = Words::with(|off| match off {
            0x04 => 0x60e9,
            0x10 => 0x11,
            0x18 => 0x105,
            0x34 => 0x10,
            0x38 => 0,
            other => stock(other),
        });
        assert_eq!(compare(&bus).ndiffs, 5);
        let mut pauses = Vec::new();
        let set = set_stock(&bus, |ms| pauses.push(ms));
        assert_eq!(set, Set { writes: 4, failed: None, pll_wait_ms: 0 });
        assert!(pauses.is_empty(), "the PLL was locked: no wait");
        assert_eq!(
            *bus.1.borrow(),
            vec![(0x04, 0x60ef), (0x10, 0x15), (0x18, 0x104), (0x34, 0x1c)]
        );
        let after = compare(&bus);
        assert_eq!((after.matches, after.diffs()), (20, &[(0x38, 0)][..]), "the status word");
        assert_eq!(set_stock(&bus, |_| {}).writes, 0, "nothing left to write");
        // The compared-only words are never written, whatever they read.
        let unmeasured = Words::with(|off| if off == 0x8c { 0 } else { stock(off) });
        assert_eq!(set_stock(&unmeasured, |_| {}).writes, 0);
        assert_eq!(compare(&unmeasured).diffs(), &[(0x8c, 0)][..]);
    }

    /// A cold PHY: the divider word is written first, the PLL's lock waited for on the pause
    /// (the file locks on the fourth read of the reset/mode word: the compare, the first poll,
    /// one more — two pauses), then the rest in order.
    #[test]
    fn a_cold_phy_gets_the_divider_first_and_the_lock_is_waited_for() {
        struct Locks(Words, std::cell::Cell<u32>);
        impl Bus for Locks {
            fn read(&self, addr: usize) -> u32 {
                if addr == RST_MODE_CTRL && self.0.read(PLL_DIV_CFG) == 0xbec4 {
                    let polls = self.1.get() + 1;
                    self.1.set(polls);
                    return self.0.read(addr) | u32::from(polls > 3);
                }
                self.0.read(addr)
            }
            fn write(&self, addr: usize, value: u32) {
                self.0.write(addr, value);
            }
        }
        let bus = Locks(Words::with(|_| 0), std::cell::Cell::new(0));
        let mut pauses = Vec::new();
        let set = set_stock(&bus, |ms| pauses.push(ms));
        assert_eq!(set, Set { writes: 12, failed: None, pll_wait_ms: 10 });
        assert_eq!(pauses, vec![5, 5]);
        let writes = bus.0 .1.borrow();
        assert_eq!(writes[0], (PLL_DIV_CFG, 0xbec4));
        assert_eq!(writes[1], (RST_MODE_CTRL, 0x60ef));
        assert_eq!(writes.len(), 12, "every writable word, the status and compared ones never");
    }

    /// A PLL that never locks stops the step at the reset/mode word after the bounded wait; a
    /// word that will not take the write stops it and names itself.
    #[test]
    fn test_reject_a_pll_that_never_locks_and_a_word_that_does_not_read_back() {
        let dead = Words::with(|_| 0);
        let mut waited = 0;
        let set = set_stock(&dead, |ms| waited += ms);
        assert_eq!(set, Set { writes: 1, failed: Some((RST_MODE_CTRL, 0)), pll_wait_ms: 50 });
        assert_eq!(waited, 50);
        struct Stuck;
        impl Bus for Stuck {
            fn read(&self, addr: usize) -> u32 {
                if addr == 0x04 {
                    0x60e9
                } else {
                    stock(addr)
                }
            }
            fn write(&self, _: usize, _: u32) {}
        }
        assert_eq!(
            set_stock(&Stuck, |_| {}),
            Set { writes: 1, failed: Some((0x04, 0x60e9)), pll_wait_ms: 0 }
        );
    }
}
