// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: An eMMC as JEDEC describes it, the parts the driver speaks: the states, the power-up
//! handshake, CID/CSD/EXT_CSD, `SWITCH` into the EXT_CSD's writable bytes, pre-defined
//! multi-block transfers (CMD23 then CMD18/25), stop and status. R1 reports the state the
//! card was in when it took the command; error bits are reported once, then clear. Sectors
//! nobody wrote read as a pattern of their address, so a read proves it fetched the sector
//! it was asked for.
//! OWNERS: @runtime @drivers

use std::collections::BTreeMap;

use storage_sdhci::proto::{ext, status, OCR_ACCESS_MASK, OCR_READY, OCR_SECTOR, OCR_VOLTAGES};

/// States (`CURRENT_STATE`).
pub const IDLE: u8 = 0;
pub const READY: u8 = 1;
pub const IDENT: u8 = 2;
pub const STANDBY: u8 = 3;
pub const TRANSFER: u8 = 4;
pub const DATA: u8 = 5;
pub const RECEIVE: u8 = 6;

/// What the card answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// No response (CMD0).
    None,
    /// A card status.
    R1(u32),
    /// A CID or CSD.
    R2(u128),
    /// The OCR.
    R3(u32),
    /// Nothing: the command was not legal here (reported in the next status).
    Silent,
}

/// A data phase the card has started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The EXT_CSD.
    ExtCsd,
    /// Sectors from `lba`, `count` of them.
    Read { lba: u32, count: u16 },
    /// Sectors to `lba`.
    Write { lba: u32, count: u16 },
}

pub struct Emmc {
    pub state: u8,
    pub ext_csd: [u8; 512],
    pub sector_mode: bool,
    /// CMD1 answers busy this many times first.
    pub busy_polls: u32,
    polls: u32,
    rca: u32,
    count: Option<u16>,
    errors: u32,
    /// A `SWITCH` of this EXT_CSD byte is refused.
    pub refuse: Option<u8>,
    /// The volatile cache survives CMD0 (a card that breaks JEDEC's E_P rule).
    pub cache_survives_cmd0: bool,
    pub written: BTreeMap<u32, [u8; 512]>,
}

pub fn pattern(lba: u32, i: usize) -> u8 {
    (lba.wrapping_mul(31) as usize + i * 7) as u8
}

pub const CID: u128 = (0x15u128 << 120) | (0x0100u128 << 104) | (0x414A_5444_3452u128 << 56);
pub const CSD: u128 = (3u128 << 126) | (4u128 << 122);

impl Emmc {
    pub fn new(ext_csd: [u8; 512]) -> Self {
        Self {
            state: IDLE,
            ext_csd,
            sector_mode: true,
            busy_polls: 3,
            polls: 0,
            rca: 0,
            count: None,
            errors: 0,
            refuse: None,
            cache_survives_cmd0: false,
            written: BTreeMap::new(),
        }
    }

    pub fn sectors(&self) -> u32 {
        let s = ext::SEC_COUNT;
        u32::from_le_bytes(self.ext_csd[s..s + 4].try_into().unwrap())
    }

    /// The bus width the card drives (from `BUS_WIDTH`) and whether it is DDR with strobe.
    pub fn bus(&self) -> (u8, bool, bool) {
        let w = self.ext_csd[usize::from(ext::BUS_WIDTH)];
        let width = match w & 0x7 {
            0 => 1,
            1 | 5 => 4,
            _ => 8,
        };
        (width, w & 0x7 >= 5, w & ext::STROBE != 0)
    }

    /// The highest clock the card's timing allows.
    pub fn max_hz(&self) -> u32 {
        match self.ext_csd[usize::from(ext::HS_TIMING)] & 0xF {
            0 => 26_000_000,
            1 => 52_000_000,
            _ => 200_000_000,
        }
    }

    fn r1(&mut self, before: u8) -> Reply {
        let status = self.errors | (u32::from(before) << 9) | status::READY_FOR_DATA;
        self.errors = 0;
        Reply::R1(status)
    }

    fn illegal(&mut self) -> Reply {
        self.errors |= status::ILLEGAL_COMMAND;
        Reply::Silent
    }

    /// Take a command; returns the reply and the data phase it starts.
    pub fn command(&mut self, index: u8, arg: u32) -> (Reply, Option<Phase>) {
        let before = self.state;
        let addressed = arg >> 16 == self.rca && self.rca != 0;
        let reply = match (index, self.state) {
            (0, _) => {
                // The bus mode and the cache are reset by CMD0 (JEDEC type E_P).
                (self.state, self.polls, self.count) = (IDLE, 0, None);
                for at in [ext::BUS_WIDTH, ext::HS_TIMING, ext::CACHE_CTRL] {
                    if at != ext::CACHE_CTRL || !self.cache_survives_cmd0 {
                        self.ext_csd[usize::from(at)] = 0;
                    }
                }
                Reply::None
            }
            (1, IDLE | READY) => {
                self.polls += 1;
                let mode = if self.sector_mode { OCR_SECTOR } else { 0 };
                let mut ocr = OCR_VOLTAGES | (mode & OCR_ACCESS_MASK);
                if self.polls > self.busy_polls {
                    self.state = READY;
                    ocr |= OCR_READY;
                }
                Reply::R3(ocr)
            }
            (2, READY) => {
                self.state = IDENT;
                Reply::R2(CID)
            }
            (3, IDENT) => {
                self.rca = arg >> 16;
                self.state = STANDBY;
                self.r1(before)
            }
            (9, STANDBY) if addressed => Reply::R2(CSD),
            (7, STANDBY) if addressed => {
                self.state = TRANSFER;
                self.r1(before)
            }
            (13, _) if addressed => self.r1(before),
            (8, TRANSFER) => {
                self.state = DATA;
                return (self.r1(before), Some(Phase::ExtCsd));
            }
            (6, TRANSFER) => {
                // A refused switch shows in the NEXT status (JEDEC: SWITCH_ERROR is "E X").
                let reply = self.r1(before);
                let (access, at, value) = ((arg >> 24) & 3, (arg >> 16) as u8, (arg >> 8) as u8);
                if access != 3 || at >= 192 || self.refuse == Some(at) {
                    self.errors |= status::SWITCH_ERROR;
                } else {
                    self.ext_csd[usize::from(at)] = value;
                }
                reply
            }
            (23, TRANSFER) => {
                self.count = Some((arg & 0xFFFF) as u16);
                self.r1(before)
            }
            (18 | 25, TRANSFER) => {
                let Some(count) = self.count.take() else { return (self.illegal(), None) };
                if u64::from(arg) + u64::from(count) > u64::from(self.sectors()) {
                    self.errors |= status::OUT_OF_RANGE;
                    return (self.r1(before), None);
                }
                let (state, phase) = if index == 18 {
                    (DATA, Phase::Read { lba: arg, count })
                } else {
                    (RECEIVE, Phase::Write { lba: arg, count })
                };
                self.state = state;
                return (self.r1(before), Some(phase));
            }
            (12, DATA | RECEIVE) => {
                self.state = TRANSFER;
                self.r1(before)
            }
            _ => self.illegal(),
        };
        (reply, None)
    }

    /// The data phase ended (the controller moved every block, or stopped).
    pub fn phase_done(&mut self) {
        if matches!(self.state, DATA | RECEIVE) {
            self.state = TRANSFER;
        }
    }

    pub fn read_sector(&self, lba: u32) -> [u8; 512] {
        self.written.get(&lba).copied().unwrap_or_else(|| {
            let mut s = [0u8; 512];
            for (i, b) in s.iter_mut().enumerate() {
                *b = pattern(lba, i);
            }
            s
        })
    }
}
