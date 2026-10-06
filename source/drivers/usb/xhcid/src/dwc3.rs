// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The DWC3 core around the board's xHCI (TASK-0328 U3, RFC-0099 §6): its global registers lie
//! in the same window as the xHCI (`0xc100` on), and three of them decide whether the xHCI
//! sees a bus at all — the port capability must say host, and the PHY suspend bits the stock
//! tree disables by quirk (`dis_u2_susphy`, `dis_enblslpm`, `dis_u3_susphy`) must be clear.
//! Measured on the stock system in host mode (2026-10-04, `docs/board/measurements/
//! 2026-10-04-usb-topology/`): `GSNPSID 0x5533330a` (a DWC3 3.30a), `GCTL 0x32c91004`
//! (PRTCAPDIR = 1, host), `GUSB2PHYCFG 0x40102400` (bits 6 and 8 clear), `GUSB3PIPECTL
//! 0x01080003` (bit 17 clear). [`host_mode`] makes exactly those fields so, read back, and
//! leaves every other bit as the reset left it; a window that is no DWC3 is refused before any
//! write. Pure over `nexus_hal::Bus`, host-tested.

use nexus_hal::Bus;

/// The global registers' offsets inside the controller window.
pub const GSNPSID: usize = 0xc120;
pub const GCTL: usize = 0xc110;
pub const GUSB2PHYCFG: usize = 0xc200;
pub const GUSB3PIPECTL: usize = 0xc2c0;

/// `GSNPSID`'s upper half for every DWC3 (`0x5533` = "U3").
const DWC3_ID: u32 = 0x5533;
/// `GCTL.PRTCAPDIR` (bits 13:12): 1 = host.
const PRTCAPDIR_MASK: u32 = 0x3 << 12;
const PRTCAPDIR_HOST: u32 = 0x1 << 12;
/// `GUSB2PHYCFG.SUSPHY` (bit 6) and `ENBLSLPM` (bit 8): a PHY the core may suspend.
const USB2_SUSPHY: u32 = 1 << 6;
const USB2_ENBLSLPM: u32 = 1 << 8;
/// `GUSB3PIPECTL.SUSPHY` (bit 17) and the PHY power-change delay (bit 18) the stock tree
/// disables by quirk (`dis-del-phy-power-chg`; the stock word has it clear, ours came up set).
const USB3_SUSPHY: u32 = 1 << 17;
const USB3_DELAY_PHY_PWR_CHG: u32 = 1 << 18;
/// `GCTL.CORESOFTRESET` (bit 11) and the PHY soft resets (bit 31 of either PHY word).
const CORE_SOFT_RESET: u32 = 1 << 11;
const PHY_SOFT_RESET: u32 = 1 << 31;
/// The core soft reset's two holds (the dwc3 driver's `mdelay(100)` twice).
pub const SOFT_RESET_HOLD_MS: u32 = 100;

/// What [`host_mode`] found and left: each word before and after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dwc3 {
    pub id: u32,
    pub gctl: (u32, u32),
    pub usb2phycfg: (u32, u32),
    pub usb3pipectl: (u32, u32),
    /// Words written (0: the core was in host mode already).
    pub writes: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dwc3Error {
    /// `GSNPSID` is no DWC3's (the window is something else): nothing written.
    NotADwc3(u32),
    /// A write did not read back (the register and the word read).
    ReadBack(usize, u32),
}

/// Put the core in host mode with its PHYs never suspended; every write read back.
pub fn host_mode<B: Bus>(bus: &B) -> Result<Dwc3, Dwc3Error> {
    let id = bus.read(GSNPSID);
    if id >> 16 != DWC3_ID {
        return Err(Dwc3Error::NotADwc3(id));
    }
    let mut writes = 0u8;
    let gctl = set(bus, GCTL, PRTCAPDIR_MASK, PRTCAPDIR_HOST, &mut writes)?;
    let usb2phycfg = set(bus, GUSB2PHYCFG, USB2_SUSPHY | USB2_ENBLSLPM, 0, &mut writes)?;
    let usb3pipectl = set(bus, GUSB3PIPECTL, USB3_SUSPHY | USB3_DELAY_PHY_PWR_CHG, 0, &mut writes)?;
    Ok(Dwc3 { id, gctl, usb2phycfg, usb3pipectl, writes })
}

/// The core soft reset as the dwc3 driver does it before anything else (board cycle 4: the
/// xHCI's reset never completed with the core as the strap left it): the core and both PHYs
/// held in soft reset, the PHYs released after a hold, the core after another — `hold` is the
/// caller's bounded wait (a kernel one-shot, never a spin). Every write read back.
pub fn core_soft_reset<B: Bus>(bus: &B, mut hold: impl FnMut(u32)) -> Result<(), Dwc3Error> {
    let id = bus.read(GSNPSID);
    if id >> 16 != DWC3_ID {
        return Err(Dwc3Error::NotADwc3(id));
    }
    let mut writes = 0u8;
    set(bus, GCTL, CORE_SOFT_RESET, CORE_SOFT_RESET, &mut writes)?;
    set(bus, GUSB3PIPECTL, PHY_SOFT_RESET, PHY_SOFT_RESET, &mut writes)?;
    set(bus, GUSB2PHYCFG, PHY_SOFT_RESET, PHY_SOFT_RESET, &mut writes)?;
    hold(SOFT_RESET_HOLD_MS);
    set(bus, GUSB3PIPECTL, PHY_SOFT_RESET, 0, &mut writes)?;
    set(bus, GUSB2PHYCFG, PHY_SOFT_RESET, 0, &mut writes)?;
    hold(SOFT_RESET_HOLD_MS);
    set(bus, GCTL, CORE_SOFT_RESET, 0, &mut writes)?;
    Ok(())
}

/// Make `mask` of the word at `reg` read `want`; returns the word before and after.
fn set<B: Bus>(
    bus: &B,
    reg: usize,
    mask: u32,
    want: u32,
    writes: &mut u8,
) -> Result<(u32, u32), Dwc3Error> {
    let before = bus.read(reg);
    if before & mask == want {
        return Ok((before, before));
    }
    bus.write(reg, (before & !mask) | want);
    let after = bus.read(reg);
    if after & mask != want {
        return Err(Dwc3Error::ReadBack(reg, after));
    }
    *writes += 1;
    Ok((before, after))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A register file: what the stock system runs with, or a reset-default core.
    struct Regs(RefCell<HashMap<usize, u32>>, RefCell<Vec<(usize, u32)>>);

    impl Regs {
        fn with(words: &[(usize, u32)]) -> Self {
            Regs(RefCell::new(words.iter().copied().collect()), RefCell::new(Vec::new()))
        }
    }

    impl Bus for Regs {
        fn read(&self, addr: usize) -> u32 {
            self.0.borrow().get(&addr).copied().unwrap_or(0)
        }
        fn write(&self, addr: usize, value: u32) {
            self.1.borrow_mut().push((addr, value));
            self.0.borrow_mut().insert(addr, value);
        }
    }

    const STOCK: [(usize, u32); 4] = [
        (GSNPSID, 0x5533_330a),
        (GCTL, 0x32c9_1004),
        (GUSB2PHYCFG, 0x4010_2400),
        (GUSB3PIPECTL, 0x0108_0003),
    ];

    #[test]
    fn the_stock_core_is_in_host_mode_already_and_nothing_is_written() {
        let bus = Regs::with(&STOCK);
        let core = host_mode(&bus).unwrap();
        assert_eq!(core.id, 0x5533_330a);
        assert_eq!(core.writes, 0);
        assert!(bus.1.borrow().is_empty());
        assert_eq!(core.gctl, (0x32c9_1004, 0x32c9_1004));
    }

    /// A core the strap left in device mode with its PHYs suspendable: exactly the three
    /// fields change, every other bit stays.
    #[test]
    fn a_core_in_device_mode_gets_exactly_the_three_fields() {
        let bus = Regs::with(&[
            (GSNPSID, 0x5533_330a),
            (GCTL, 0x32c9_2004),
            (GUSB2PHYCFG, 0x4010_2540),
            (GUSB3PIPECTL, 0x010e_0003),
        ]);
        let core = host_mode(&bus).unwrap();
        assert_eq!(core.writes, 3);
        assert_eq!(
            *bus.1.borrow(),
            vec![(GCTL, 0x32c9_1004), (GUSB2PHYCFG, 0x4010_2400), (GUSB3PIPECTL, 0x0108_0003)]
        );
        assert_eq!(core.usb2phycfg, (0x4010_2540, 0x4010_2400));
    }

    /// The soft reset's six writes in the driver's order, the two holds between them.
    #[test]
    fn the_core_soft_reset_holds_twice_and_reads_every_write_back() {
        let bus = Regs::with(&STOCK);
        let mut holds = Vec::new();
        core_soft_reset(&bus, |ms| holds.push(ms)).unwrap();
        assert_eq!(holds, [100, 100]);
        assert_eq!(
            *bus.1.borrow(),
            vec![
                (GCTL, 0x32c9_1004 | (1 << 11)),
                (GUSB3PIPECTL, 0x0108_0003 | (1 << 31)),
                (GUSB2PHYCFG, 0x4010_2400 | (1 << 31)),
                (GUSB3PIPECTL, 0x0108_0003),
                (GUSB2PHYCFG, 0x4010_2400),
                (GCTL, 0x32c9_1004),
            ]
        );
        assert_eq!(
            core_soft_reset(&Regs::with(&[(GSNPSID, 1)]), |_| {}),
            Err(Dwc3Error::NotADwc3(1))
        );
    }

    #[test]
    fn test_reject_a_window_that_is_no_dwc3_before_any_write() {
        let bus = Regs::with(&[(GSNPSID, 0x0110_0020)]);
        assert_eq!(host_mode(&bus), Err(Dwc3Error::NotADwc3(0x0110_0020)));
        assert!(bus.1.borrow().is_empty());
    }

    /// A word that will not take the write (a gated block reads zeros): the fault names it.
    #[test]
    fn test_reject_a_write_that_does_not_read_back() {
        struct Dead;
        impl Bus for Dead {
            fn read(&self, addr: usize) -> u32 {
                if addr == GSNPSID {
                    0x5533_330a
                } else {
                    0
                }
            }
            fn write(&self, _: usize, _: u32) {}
        }
        assert_eq!(host_mode(&Dead), Err(Dwc3Error::ReadBack(GCTL, 0)));
    }
}
