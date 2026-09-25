// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A standard SDHCI controller (3.00, the parts the driver uses) plus, optionally, the K1's
//! vendor registers — accessed as the driver accesses it, aligned 32-bit words; anything
//! else fails the test. Commands and data phases complete the moment they are issued (the
//! model has no time of its own), but only when the host set up what the bus needs: power,
//! a running card clock, identification at 400 kHz, never a clock above the card's timing,
//! the width and the DDR/strobe mode the card was switched to, and on the K1 the PHY, the
//! pads, HS400 and the locked DLL. A violation fails as the bus would: no response, a CRC
//! error. The ADMA2 engine follows the descriptors through DRAM (an END before the block
//! count or a count before END is a length mismatch).
//! OWNERS: @runtime @drivers

use std::collections::BTreeMap;

use storage_sdhci::adma::{ATTR_END, ATTR_TRAN, ATTR_VALID, DESC_BYTES};
use storage_sdhci::k1;
use storage_sdhci::regs::*;

use super::card::{Phase, Reply, DATA, RECEIVE};
use super::{Event, Machine};

pub struct Ctrl {
    pub version: u8,
    pub caps: u32,
    pub base_hz: u32,
    pub k1: bool,
    block: u32,
    arg: u32,
    resp: [u32; 4],
    host_ctrl: u32,
    clock: u32,
    status: u32,
    enable: u32,
    signal: u32,
    host_ctrl2: u32,
    adma_error: u32,
    adma_addr: u32,
    vendor: BTreeMap<usize, u32>,
    /// A PIO read in progress: the bytes, how far the driver read, the blocks it gets.
    pio: Option<(Vec<u8>, usize, u16)>,
}

impl Ctrl {
    pub fn new(version: u8, caps: u32, base_hz: u32, k1: bool) -> Self {
        Self {
            version,
            caps,
            base_hz,
            k1,
            block: 0,
            arg: 0,
            resp: [0; 4],
            host_ctrl: 0,
            clock: 0,
            status: 0,
            enable: 0,
            signal: 0,
            host_ctrl2: 0,
            adma_error: 0,
            adma_addr: 0,
            vendor: BTreeMap::new(),
            pio: None,
        }
    }

    /// A full reset: every standard register to its default; the vendor ones keep their
    /// values (the worst case for stale mode bits).
    fn reset_all(&mut self) {
        let vendor = std::mem::take(&mut self.vendor);
        *self = Self { vendor, ..Self::new(self.version, self.caps, self.base_hz, self.k1) };
    }

    fn vendor(&self, off: usize) -> u32 {
        self.vendor.get(&off).copied().unwrap_or(0)
    }

    /// An interrupt is pending on an enabled status bit.
    pub fn pending(&self) -> bool {
        self.status & self.enable != 0
    }

    /// Whether the driver lets the line carry the status.
    pub fn signalled(&self) -> u32 {
        self.signal
    }

    /// The card clock the divider makes, or 0 when it is stopped.
    pub fn card_hz(&self) -> u32 {
        if self.clock & (CLK_INT_EN | CLK_CARD_EN) != CLK_INT_EN | CLK_CARD_EN {
            return 0;
        }
        let n = ((self.clock >> 8) & 0xFF) | (((self.clock >> 6) & 3) << 8);
        let divisor = if self.version >= SPEC_300 { 2 * n } else { 2 * ((self.clock >> 8) & 0xFF) };
        if divisor == 0 {
            self.base_hz
        } else {
            self.base_hz / divisor
        }
    }

    pub fn width(&self) -> u8 {
        if self.host_ctrl & HC1_WIDTH_8 != 0 {
            8
        } else if self.host_ctrl & HC1_WIDTH_4 != 0 {
            4
        } else {
            1
        }
    }
}

fn r2_words(v: u128) -> [u32; 4] {
    [(v >> 8) as u32, (v >> 40) as u32, (v >> 72) as u32, ((v >> 104) as u32) & 0x00FF_FFFF]
}

impl Machine {
    fn latch(&mut self, bits: u32) {
        self.ctrl.status |= bits & self.ctrl.enable;
    }

    fn dll_locked(&self) -> bool {
        let (cfg, cfg1) = (self.ctrl.vendor(k1::PHY_DLLCFG), self.ctrl.vendor(k1::PHY_DLLCFG1));
        cfg & k1::DLL_ENABLE != 0
            && cfg & k1::DLL_FIELDS == k1::DLL_FIELDS_1
            && cfg1 & k1::DLL_REG1_MASK == k1::DLL_REG1
            && !self.faults.dll_never_locks
    }

    /// Power, a running and stable card clock, and on the K1 the PHY, the card mode and
    /// the pad clock: without them nobody answers.
    fn bus_up(&self) -> bool {
        let c = &self.ctrl;
        let k1_up = !c.k1
            || (c.vendor(k1::PHY_CTRL) & (k1::PHY_FUNC_EN | k1::PHY_PLL_LOCK) != 0
                && c.vendor(k1::MMC_CTRL) & k1::MMC_CARD_MODE != 0
                && c.vendor(k1::LEGACY_CTRL) & k1::GEN_PAD_CLK_ON != 0);
        c.host_ctrl & PWR_ON != 0 && c.card_hz() > 0 && !self.faults.clock_never_stable && k1_up
    }

    /// Host and card agree on the data bus: width, DDR with strobe, high-speed sampling, and
    /// on the K1 the HS400 mode, the strobe and the DLL, and the drive strength.
    fn data_bus_ok(&self) -> bool {
        let c = &self.ctrl;
        let (width, ddr, strobe) = self.card.bus();
        let hs400 = c.host_ctrl2 & HC2_UHS_MASK == HC2_UHS_HS400;
        let high_speed = c.host_ctrl & HC1_HIGH_SPEED != 0 || c.card_hz() <= 26_000_000;
        let k1_ok = !c.k1 || {
            let mmc = c.vendor(k1::MMC_CTRL);
            let pad = c.vendor(k1::PHY_PADCFG);
            let tx_int = c.vendor(k1::TX_CFG) & k1::TX_INT_CLK_SEL != 0;
            let drive = pad & k1::PHY_DRIVE_MASK == k1::PHY_DRIVE && pad & k1::RX_BIAS_CTRL != 0;
            let modes = if hs400 {
                mmc & (k1::MMC_HS400 | k1::ENHANCE_STROBE_EN)
                    == k1::MMC_HS400 | k1::ENHANCE_STROBE_EN
                    && self.dll_locked()
                    && !tx_int
            } else {
                mmc & (k1::MMC_HS400 | k1::ENHANCE_STROBE_EN) == 0 && tx_int
            };
            drive && modes
        };
        width == c.width() && ddr == hs400 && strobe == hs400 && high_speed && k1_ok
    }

    pub fn read(&mut self, off: usize) -> u32 {
        let c = &self.ctrl;
        match off {
            BLOCK => c.block,
            o if (RESPONSE..RESPONSE + 16).contains(&o) => c.resp[(o - RESPONSE) / 4],
            DATA_PORT => self.pio_read(),
            PRESENT => 0,
            HOST_CTRL => c.host_ctrl,
            CLOCK => {
                let stable = c.clock & CLK_INT_EN != 0 && !self.faults.clock_never_stable;
                c.clock | if stable { CLK_INT_STABLE } else { 0 }
            }
            INT_STATUS => c.status | if c.status & ERR_ALL != 0 { 1 << 15 } else { 0 },
            HOST_CTRL2 => c.host_ctrl2,
            CAPS => c.caps,
            ADMA_ERROR => c.adma_error,
            VERSION => u32::from(c.version) << 16,
            k1::PHY_DLLSTS if c.k1 => u32::from(self.dll_locked()),
            o if c.k1 && (0x100..0x200).contains(&o) => c.vendor(o),
            _ => panic!("read of a register the model does not have: 0x{off:x}"),
        }
    }

    pub fn write(&mut self, off: usize, v: u32) {
        match off {
            BLOCK => self.ctrl.block = v,
            ARGUMENT => self.ctrl.arg = v,
            XFER_CMD => self.issue(v),
            HOST_CTRL => self.ctrl.host_ctrl = v,
            CLOCK if v & RESET_ALL != 0 => {
                self.ctrl.reset_all();
                if self.faults.reset_never_clears {
                    self.ctrl.clock = RESET_ALL;
                }
            }
            CLOCK => {
                if v & RESET_DATA != 0 {
                    self.ctrl.pio = None;
                }
                self.ctrl.clock = v & !(RESET_MASK | CLK_INT_STABLE);
            }
            INT_STATUS => self.ctrl.status &= !v,
            INT_ENABLE => self.ctrl.enable = v,
            INT_SIGNAL => self.ctrl.signal = v,
            HOST_CTRL2 => self.ctrl.host_ctrl2 = v & 0xFFFF_0000,
            ADMA_ADDR => self.ctrl.adma_addr = v,
            ADMA_ADDR_HI => assert_eq!(v, 0, "32-bit descriptors only"),
            o if self.ctrl.k1 && (0x100..0x200).contains(&o) => {
                self.ctrl.vendor.insert(o, v);
                self.log.push(Event::Vendor(o, v));
            }
            _ => panic!("write of a register the model does not have: 0x{off:x} = 0x{v:x}"),
        }
    }

    fn issue(&mut self, word: u32) {
        let (mode, cmd) = (word & 0xFFFF, word >> 16);
        let index = ((cmd >> 8) & 0x3F) as u8;
        let (resp, data) = (cmd & 3, cmd & CMD_DATA != 0);
        let arg = self.ctrl.arg;
        self.log.push(Event::Cmd(index, arg));
        if self.faults.never_complete == Some(index) {
            return;
        }
        let hz = self.ctrl.card_hz();
        let ident = matches!(index, 1..=3);
        if !self.bus_up() || (ident && hz > 400_000) || self.faults.silent == Some(index) {
            return self.latch(ERR_CMD_TIMEOUT);
        }
        if hz > self.card.max_hz() || self.faults.crc == Some(index) {
            return self.latch(ERR_CMD_CRC);
        }
        let (reply, phase) = self.card.command(index, arg);
        self.ctrl.resp = match (reply, resp) {
            (Reply::Silent, _) => return self.latch(ERR_CMD_TIMEOUT),
            (Reply::None, 0) => [0; 4],
            (Reply::R1(s), 2 | 3) => [self.inject(index, s), 0, 0, 0],
            (Reply::R3(ocr), 2) => [ocr, 0, 0, 0],
            (Reply::R2(v), 1) => r2_words(v),
            (reply, resp) => panic!("CMD{index}: response type {resp} for {reply:?}"),
        };
        self.latch(INT_CMD_COMPLETE);
        if resp == 3 {
            self.latch(INT_XFER_COMPLETE);
        }
        match (data, phase) {
            (false, None) => {}
            (true, None) => self.latch(ERR_DATA_TIMEOUT),
            (true, Some(phase)) => self.data_phase(mode, phase),
            (false, Some(phase)) => panic!("CMD{index} starts {phase:?} but carries no data flag"),
        }
    }

    fn inject(&self, index: u8, status: u32) -> u32 {
        let mut s = status;
        if let Some((cmd, bits)) = self.faults.status_bits {
            if cmd == index {
                s |= bits;
            }
        }
        if let Some((cmd, state)) = self.faults.stuck_state {
            if cmd == index {
                s = (s & !(0xF << 9)) | (u32::from(state) << 9);
            }
        }
        s
    }

    fn data_phase(&mut self, mode: u32, phase: Phase) {
        let (count, size) = ((self.ctrl.block >> 16) as u16, self.ctrl.block & 0xFFF);
        assert_eq!(size, 512, "512-byte blocks");
        let read = mode & TM_READ != 0;
        assert!(mode & TM_BLOCK_COUNT != 0 && (count > 1) == (mode & TM_MULTI != 0));
        if std::mem::take(&mut self.faults.data_timeout) {
            return self.latch(ERR_DATA_TIMEOUT);
        }
        if !self.data_bus_ok() {
            return self.latch(ERR_DATA_CRC);
        }
        let mut bytes = match phase {
            Phase::ExtCsd => {
                assert!(read && count == 1);
                self.card.ext_csd.to_vec()
            }
            Phase::Read { lba, count: n } => {
                assert!(read && n == count);
                (lba..lba + u32::from(n)).flat_map(|l| self.card.read_sector(l)).collect()
            }
            Phase::Write { count: n, .. } => {
                assert!(!read && n == count);
                vec![0; usize::from(n) * 512]
            }
        };
        if read && self.faults.corrupt_wide && self.ctrl.width() > 1 {
            bytes.iter_mut().for_each(|b| *b ^= 0x5A);
        }
        let short = std::mem::take(&mut self.faults.short_by).min(count);
        let moved = count - short;
        if mode & TM_DMA != 0 {
            self.adma(read, &mut bytes, usize::from(moved) * 512);
            if self.ctrl.status & ERR_ADMA != 0 {
                return;
            }
            if let Phase::Write { lba, .. } = phase {
                for (i, s) in bytes.chunks_exact(512).take(usize::from(moved)).enumerate() {
                    self.card.written.insert(lba + i as u32, s.try_into().unwrap());
                }
            }
            self.finish(short);
        } else {
            assert!(read, "PIO writes are not driven");
            self.ctrl.pio = Some((bytes, 0, moved));
            self.latch(INT_BUF_READ);
        }
    }

    /// The transfer ended: the residual block count, the card back to transfer (unless it
    /// still owes blocks), transfer complete.
    fn finish(&mut self, short: u16) {
        self.ctrl.block = (self.ctrl.block & 0xFFFF) | (u32::from(short) << 16);
        if short == 0 {
            self.card.phase_done();
        } else {
            assert!(matches!(self.card.state, DATA | RECEIVE));
        }
        self.latch(INT_XFER_COMPLETE);
    }

    /// Follow the descriptors: move `len` bytes between `bytes` and DRAM.
    fn adma(&mut self, read: bool, bytes: &mut [u8], len: usize) {
        assert_eq!(self.ctrl.host_ctrl & HC1_DMA_MASK, HC1_ADMA2_32, "ADMA2 selected");
        let mut at = u64::from(self.ctrl.adma_addr);
        let mut done = 0usize;
        let fault = std::mem::take(&mut self.faults.adma_error);
        loop {
            let mut d = [0u8; DESC_BYTES];
            if fault || self.mem.read(at, &mut d).is_none() {
                return self.adma_fail(1);
            }
            let attr = u16::from_le_bytes([d[0], d[1]]);
            let piece = match u16::from_le_bytes([d[2], d[3]]) {
                0 => 65_536,
                n => usize::from(n),
            };
            let addr = u64::from(u32::from_le_bytes([d[4], d[5], d[6], d[7]]));
            if attr & ATTR_VALID == 0 || attr & 0x30 != ATTR_TRAN {
                return self.adma_fail(1);
            }
            if addr % 4 != 0 || piece % 4 != 0 {
                return self.adma_fail(1);
            }
            let take = piece.min(len - done);
            let moved = if read {
                self.mem.write(addr, &bytes[done..done + take])
            } else {
                self.mem.read(addr, &mut bytes[done..done + take])
            };
            if moved.is_none() {
                return self.adma_fail(1);
            }
            done += take;
            let end = attr & ATTR_END != 0;
            if done == len {
                if len < bytes.len() {
                    return; // the card stopped early: a short transfer, not a table error
                }
                if end && take == piece {
                    return;
                }
                return self.adma_fail(2); // the count ran out before END
            }
            if end {
                return self.adma_fail(2); // END before the count ran out
            }
            at += DESC_BYTES as u64;
        }
    }

    fn adma_fail(&mut self, state: u32) {
        self.ctrl.adma_error = state;
        self.latch(ERR_ADMA);
    }

    fn pio_read(&mut self) -> u32 {
        let (bytes, pos, blocks) =
            self.ctrl.pio.as_mut().expect("data port read without a PIO transfer");
        let word = u32::from_le_bytes(bytes[*pos..*pos + 4].try_into().unwrap());
        *pos += 4;
        if *pos % 512 == 0 {
            let (read, blocks) = (*pos / 512, usize::from(*blocks));
            let count = bytes.len() / 512;
            if read == blocks {
                self.ctrl.pio = None;
                self.finish((count - blocks) as u16);
            } else {
                self.latch(INT_BUF_READ);
            }
        }
        word
    }
}
