// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The eMMC card: from power-up to its operating mode, and its data commands. Init is
//! JEDEC's sequence — CMD0, CMD1 until the card is ready (sector addressing required), CMD2,
//! CMD3, CMD9, CMD7, the EXT_CSD by PIO at legacy speed — then the fastest mode the host,
//! the board and the card share, chosen by rule, never by trial: HS400 enhanced strobe
//! (1.8 V, high speed, `BUS_WIDTH` = 8-bit DDR with strobe, `HS_TIMING` = HS400, the host's
//! strobe and DLL, 200 MHz) or HS52 on the widest bus, or legacy. Every switch is followed
//! by the card's status; the final mode is verified by reading the EXT_CSD again over it
//! and comparing. `init_best` falls back from a failed HS400ES to HS52 from a fresh reset
//! and hands back the reason — reported, never hidden. After a data error the card is
//! stopped (CMD12) and must return to the transfer state before anything else runs.
//! OWNERS: @runtime @drivers

use nexus_hal::Bus;

use crate::cmd::{r2, Command, Resp};
use crate::engine::{blocks_of, SECTOR};
use crate::host::{Host, Timing};
use crate::proto::{self, ext, index, state, status, Cid, Csd, ExtCsd};
use crate::{Error, Platform, Stage};

/// The relative address this host gives its one card.
const RCA: u32 = 1;
/// Identification runs at or below this.
pub const IDENT_HZ: u32 = 400_000;
/// Legacy timing runs at or below this.
pub const LEGACY_HZ: u32 = 26_000_000;
/// High speed runs at or below this.
pub const HS_HZ: u32 = 52_000_000;
/// HS400 runs at or below this.
pub const HS400_HZ: u32 = 200_000_000;
/// Power-up (≥ 74 clocks) and CMD0 settle within this.
pub const POWER_UP_US: u64 = 1_000;
/// The card finishes power-up within this (JEDEC: one second).
pub const OP_COND_TIMEOUT_US: u64 = 1_000_000;
/// Between two CMD1.
pub const OP_COND_POLL_US: u64 = 1_000;
/// After an error the card is back in the transfer state within this.
pub const RECOVERY_TIMEOUT_US: u64 = 1_000_000;
/// Between two status reads while recovering.
pub const RECOVERY_POLL_US: u64 = 1_000;

/// What the card runs at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Legacy timing (up to 26 MHz, SDR) on a `width`-bit bus.
    Legacy {
        /// The bus width.
        width: u8,
    },
    /// High speed SDR up to 52 MHz on a `width`-bit bus.
    Hs52 {
        /// The bus width.
        width: u8,
    },
    /// HS400 enhanced strobe: 8 bits, DDR, up to 200 MHz.
    Hs400es,
}

/// The fastest mode `init` may choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ceiling {
    /// Anything, up to HS400 enhanced strobe.
    Hs400es,
    /// Up to HS52.
    Hs52,
    /// Legacy timing only (a reader that wants the most conservative clock).
    Legacy,
}

/// An init that failed: the host back (for another attempt) and why.
pub struct InitFailure<B: Bus, P: Platform> {
    /// The host.
    pub host: Host<B, P>,
    /// Why.
    pub error: Error,
}

/// An initialised eMMC.
pub struct Card<B: Bus, P: Platform> {
    host: Host<B, P>,
    cid: Cid,
    ext: ExtCsd,
    mode: Mode,
}

impl<B: Bus, P: Platform> Card<B, P> {
    /// Bring the card from power-up to the fastest mode at or below `ceiling` that the host,
    /// the board and the card share.
    pub fn init(mut host: Host<B, P>, ceiling: Ceiling) -> Result<Self, InitFailure<B, P>> {
        match bring_up(&mut host, ceiling) {
            Ok((cid, ext, mode)) => Ok(Self { host, cid, ext, mode }),
            Err(error) => Err(InitFailure { host, error }),
        }
    }

    /// HS400 enhanced strobe when the host can, else — or when it fails — HS52 from a fresh
    /// reset. The error that cost HS400ES comes back beside the card.
    pub fn init_best(host: Host<B, P>) -> Result<(Self, Option<Error>), InitFailure<B, P>> {
        if !host.can_hs400es() {
            return Self::init(host, Ceiling::Hs52).map(|card| (card, None));
        }
        match Self::init(host, Ceiling::Hs400es) {
            Ok(card) => Ok((card, None)),
            Err(failed) => {
                let reason = failed.error;
                Self::init(failed.host, Ceiling::Hs52).map(|card| (card, Some(reason)))
            }
        }
    }

    /// The mode the card runs at.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The capacity in 512-byte sectors.
    pub fn sectors(&self) -> u32 {
        self.ext.sectors
    }

    /// The card's identity.
    pub fn cid(&self) -> Cid {
        self.cid
    }

    /// The EXT_CSD facts read at init.
    pub fn ext_csd(&self) -> ExtCsd {
        self.ext
    }

    /// The host.
    pub fn host(&self) -> &Host<B, P> {
        &self.host
    }

    /// Read `buf.len() / 512` sectors from `lba` by PIO — no DMA, no interrupt needed (the
    /// boot loader's path).
    pub fn read_pio(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), Error> {
        let blocks = blocks_of(buf.len())?;
        self.check_range(lba, blocks)?;
        let result = self.read_pio_inner(lba, blocks, buf);
        self.recover_after(result)
    }

    fn read_pio_inner(&mut self, lba: u32, blocks: u16, buf: &mut [u8]) -> Result<(), Error> {
        self.set_block_count(blocks)?;
        let cmd = Command::new(index::READ_MULTIPLE_BLOCK, lba, Resp::R1);
        let r1 = self.host.read_pio(cmd, buf)?;
        check(r1, cmd.index, Some(state::TRANSFER))
    }

    /// Move `blocks` sectors at `lba` by ADMA2 with the descriptor table at bus address
    /// `table` (written and handed to the device by [`crate::Disk`]).
    pub(crate) fn adma(
        &mut self,
        lba: u32,
        blocks: u16,
        write: bool,
        table: u32,
    ) -> Result<(), Error> {
        self.check_range(lba, blocks)?;
        let result = self.adma_inner(lba, blocks, write, table);
        self.recover_after(result)
    }

    fn adma_inner(&mut self, lba: u32, blocks: u16, write: bool, table: u32) -> Result<(), Error> {
        self.set_block_count(blocks)?;
        let op = if write { index::WRITE_MULTIPLE_BLOCK } else { index::READ_MULTIPLE_BLOCK };
        let cmd = Command::new(op, lba, Resp::R1);
        let r1 = self.host.adma(cmd, write, blocks, table)?;
        check(r1, cmd.index, Some(state::TRANSFER))?;
        if write {
            // An error found while programming shows in the status that follows.
            card_status(&mut self.host, Some(state::TRANSFER))?;
        }
        Ok(())
    }

    fn set_block_count(&mut self, blocks: u16) -> Result<(), Error> {
        let cmd = Command::new(index::SET_BLOCK_COUNT, u32::from(blocks), Resp::R1);
        let r1 = self.host.command(cmd, 0)?[0];
        check(r1, cmd.index, Some(state::TRANSFER))
    }

    fn check_range(&self, lba: u32, blocks: u16) -> Result<(), Error> {
        match lba.checked_add(u32::from(blocks)) {
            Some(end) if end <= self.ext.sectors => Ok(()),
            _ => Err(Error::Range),
        }
    }

    /// After a failed data command: stop the card and wait until it is back in the transfer
    /// state; the first error is the one reported, unless the card cannot be recovered.
    fn recover_after(&mut self, result: Result<(), Error>) -> Result<(), Error> {
        if result.is_ok() {
            return result;
        }
        let stop = Command::new(index::STOP_TRANSMISSION, 0, Resp::R1b);
        let _ = self.host.command(stop, RECOVERY_TIMEOUT_US);
        let deadline = self.host.platform.now_us().saturating_add(RECOVERY_TIMEOUT_US);
        loop {
            let status = Command::new(index::SEND_STATUS, RCA << 16, Resp::R1);
            if let Ok([s, ..]) = self.host.command(status, 0) {
                if status::state(s) == state::TRANSFER && s & status::READY_FOR_DATA != 0 {
                    return result;
                }
            }
            if self.host.platform.now_us() >= deadline {
                return Err(Error::Timeout(Stage::Recovery));
            }
            self.host.platform.delay_us(RECOVERY_POLL_US);
        }
    }
}

/// Refuse an R1 with error bits, or taken in another state than `want`.
fn check(r1: u32, cmd: u8, want: Option<u8>) -> Result<(), Error> {
    if r1 & status::ERRORS != 0 {
        return Err(Error::CardStatus { cmd, status: r1 });
    }
    match want {
        Some(want) if status::state(r1) != want => {
            Err(Error::CardState { cmd, state: status::state(r1), want })
        }
        _ => Ok(()),
    }
}

/// CMD13: the card's status, checked.
fn card_status<B: Bus, P: Platform>(host: &mut Host<B, P>, want: Option<u8>) -> Result<u32, Error> {
    let cmd = Command::new(index::SEND_STATUS, RCA << 16, Resp::R1);
    let r1 = host.command(cmd, 0)?[0];
    check(r1, cmd.index, want)?;
    Ok(r1)
}

/// CMD6: write an EXT_CSD byte, wait out the busy, then read the status (a refused switch
/// shows there as `SWITCH_ERROR`).
fn switch<B: Bus, P: Platform>(
    host: &mut Host<B, P>,
    ext: &ExtCsd,
    at: u8,
    value: u8,
) -> Result<(), Error> {
    switch_quiet(host, ext, at, value)?;
    card_status(host, Some(state::TRANSFER)).map(|_| ())
}

/// CMD6 without the status read: for the switch after which the host must change first.
fn switch_quiet<B: Bus, P: Platform>(
    host: &mut Host<B, P>,
    ext: &ExtCsd,
    at: u8,
    value: u8,
) -> Result<(), Error> {
    let cmd = Command::new(index::SWITCH, proto::switch_arg(at, value), Resp::R1b);
    let r1 = host.command(cmd, ext.switch_us)?[0];
    check(r1, cmd.index, Some(state::TRANSFER))
}

/// CMD8 by PIO.
fn read_ext_csd<B: Bus, P: Platform>(host: &mut Host<B, P>) -> Result<[u8; 512], Error> {
    let mut raw = [0u8; SECTOR];
    let cmd = Command::new(index::SEND_EXT_CSD, 0, Resp::R1);
    let r1 = host.read_pio(cmd, &mut raw)?;
    check(r1, cmd.index, Some(state::TRANSFER))?;
    Ok(raw)
}

/// CMD1 until the card is ready.
fn op_cond<B: Bus, P: Platform>(host: &mut Host<B, P>) -> Result<u32, Error> {
    let deadline = host.platform.now_us().saturating_add(OP_COND_TIMEOUT_US);
    loop {
        let cmd = Command::new(index::SEND_OP_COND, proto::OCR_ARG, Resp::R3);
        let ocr = host.command(cmd, 0)?[0];
        if ocr & proto::OCR_READY != 0 {
            return Ok(ocr);
        }
        if host.platform.now_us() >= deadline {
            return Err(Error::Timeout(Stage::OpCond));
        }
        host.platform.delay_us(OP_COND_POLL_US);
    }
}

/// The whole init; the host's state is the caller's to hand back on failure.
fn bring_up<B: Bus, P: Platform>(
    host: &mut Host<B, P>,
    ceiling: Ceiling,
) -> Result<(Cid, ExtCsd, Mode), Error> {
    host.reset()?;
    host.set_width(1)?;
    host.set_timing(Timing::Legacy, IDENT_HZ)?;
    host.platform.delay_us(POWER_UP_US);
    host.command(Command::new(index::GO_IDLE, 0, Resp::None), 0)?;
    host.platform.delay_us(POWER_UP_US);
    let ocr = op_cond(host)?;
    if ocr & proto::OCR_VOLTAGES == 0 {
        return Err(Error::Voltage);
    }
    if ocr & proto::OCR_ACCESS_MASK != proto::OCR_SECTOR {
        return Err(Error::ByteAddressed);
    }
    let cid = Cid::from_r2(r2(host.command(Command::new(index::ALL_SEND_CID, 0, Resp::R2), 0)?));
    let cmd = Command::new(index::SET_RELATIVE_ADDR, RCA << 16, Resp::R1);
    check(host.command(cmd, 0)?[0], cmd.index, Some(state::IDENT))?;
    let csd =
        Csd::from_r2(r2(host.command(Command::new(index::SEND_CSD, RCA << 16, Resp::R2), 0)?));
    if csd.spec_vers < 4 {
        return Err(Error::CardTooOld);
    }
    let cmd = Command::new(index::SELECT_CARD, RCA << 16, Resp::R1);
    check(host.command(cmd, 0)?[0], cmd.index, Some(state::STANDBY))?;
    card_status(host, Some(state::TRANSFER))?;
    host.set_timing(Timing::Legacy, LEGACY_HZ)?;
    let raw = read_ext_csd(host)?;
    let ext = ExtCsd::parse(&raw).map_err(Error::ExtCsd)?;
    if ext.cache_on {
        // Every write is durable when it completes: no volatile cache under this driver.
        switch(host, &ext, ext::CACHE_CTRL, 0)?;
    }
    let mode = select_mode(host, &ext, ceiling)?;
    // The final bus carries the EXT_CSD intact — its read-only bytes as read at 1 bit — or
    // the mode is not usable.
    let again = read_ext_csd(host)?;
    let fixed = [
        ext::SEC_COUNT..ext::SEC_COUNT + 4,
        ext::CARD_TYPE..ext::CARD_TYPE + 1,
        ext::REV..ext::REV + 1,
    ];
    if fixed.into_iter().any(|at| again[at.clone()] != raw[at]) {
        return Err(Error::BusVerify);
    }
    Ok((cid, ext, mode))
}

/// The fastest mode at or below `ceiling` the host, the board and the card share.
fn select_mode<B: Bus, P: Platform>(
    host: &mut Host<B, P>,
    ext: &ExtCsd,
    ceiling: Ceiling,
) -> Result<Mode, Error> {
    let width = host.max_width();
    let hs52 = ext.card_type & ext::TYPE_HS52 != 0 && ceiling != Ceiling::Legacy;
    let hs400es = ceiling == Ceiling::Hs400es
        && host.can_hs400es()
        && ext.card_type & ext::TYPE_HS400_1V8 != 0
        && ext.strobe;
    if hs400es {
        host.signal_1v8()?;
        switch(host, ext, ext::HS_TIMING, ext::TIMING_HS)?;
        host.set_timing(Timing::Hs, HS_HZ)?;
        card_status(host, Some(state::TRANSFER))?;
        switch(host, ext, ext::BUS_WIDTH, ext::WIDTH_8_DDR | ext::STROBE)?;
        switch_quiet(host, ext, ext::HS_TIMING, ext::TIMING_HS400)?;
        host.set_width(8)?;
        host.set_timing(Timing::Hs400, HS400_HZ)?;
        host.enhanced_strobe()?;
        card_status(host, Some(state::TRANSFER))?;
        return Ok(Mode::Hs400es);
    }
    let bus_width = match width {
        8 => ext::WIDTH_8,
        4 => ext::WIDTH_4,
        _ => ext::WIDTH_1,
    };
    if hs52 {
        switch(host, ext, ext::HS_TIMING, ext::TIMING_HS)?;
        host.set_timing(Timing::Hs, HS_HZ)?;
        card_status(host, Some(state::TRANSFER))?;
    }
    if width > 1 {
        switch(host, ext, ext::BUS_WIDTH, bus_width)?;
        host.set_width(width)?;
    }
    Ok(if hs52 { Mode::Hs52 { width } } else { Mode::Legacy { width } })
}
