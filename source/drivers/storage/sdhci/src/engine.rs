// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The command engine: issue a command, wait for its completion on the interrupt (errors
//! first), read the response; move a data phase by PIO or ADMA2; reset the lines after an
//! error so the next command starts clean. Waits sleep on the interrupt until a deadline
//! (`Platform::wait_irq`); only the inhibit bits — which raise no interrupt — are polled.
//! OWNERS: @runtime @drivers

use nexus_hal::Bus;

use crate::cmd::{Command, Resp};
use crate::host::{poll, rmw, Host, POLL_US};
use crate::regs::*;
use crate::{Error, Platform, Stage};

/// The command and data lines are free within this before a command.
pub const INHIBIT_TIMEOUT_US: u64 = 10_000;
/// A command completes within this (the card answers within 64 clocks; the rest is margin).
pub const COMMAND_TIMEOUT_US: u64 = 100_000;
/// A read completes within this, plus [`PER_BLOCK_US`] per block.
pub const READ_TIMEOUT_US: u64 = 250_000;
/// A write (programming included) completes within this, plus [`PER_BLOCK_US`] per block.
pub const WRITE_TIMEOUT_US: u64 = 1_000_000;
/// The per-block allowance on top of a data deadline.
pub const PER_BLOCK_US: u64 = 1_000;
/// The one block size (sector-addressed eMMC).
pub const SECTOR: usize = 512;

/// The block-size word: 512-byte blocks, the largest SDMA boundary (unused), `count` blocks.
fn block_word(count: u16) -> u32 {
    (u32::from(count) << 16) | (7 << 12) | SECTOR as u32
}

impl<B: Bus, P: Platform> Host<B, P> {
    /// A command without data; returns its response words. `busy_us` bounds the busy an
    /// R1b response may hold.
    pub(crate) fn command(&mut self, cmd: Command, busy_us: u64) -> Result<[u32; 4], Error> {
        let result = self.command_inner(cmd, busy_us);
        if result.is_err() {
            let lines = if cmd.resp == Resp::R1b { RESET_CMD | RESET_DATA } else { RESET_CMD };
            let _ = self.reset_lines(lines);
        }
        result
    }

    fn command_inner(&mut self, cmd: Command, busy_us: u64) -> Result<[u32; 4], Error> {
        self.issue(&cmd, 0, false, 0)?;
        let deadline = self.platform.now_us().saturating_add(COMMAND_TIMEOUT_US);
        self.wait(INT_CMD_COMPLETE, Stage::Command, deadline, cmd.index)?;
        let resp = self.response(cmd.resp);
        if cmd.resp == Resp::R1b {
            let deadline = self.platform.now_us().saturating_add(busy_us);
            self.wait(INT_XFER_COMPLETE, Stage::Busy, deadline, cmd.index)?;
        }
        Ok(resp)
    }

    /// A read command whose data moves by PIO into `buf` (whole blocks); returns the R1.
    pub(crate) fn read_pio(&mut self, cmd: Command, buf: &mut [u8]) -> Result<u32, Error> {
        let result = self.read_pio_inner(cmd, buf);
        if result.is_err() {
            let _ = self.reset_lines(RESET_CMD | RESET_DATA);
        }
        result
    }

    fn read_pio_inner(&mut self, cmd: Command, buf: &mut [u8]) -> Result<u32, Error> {
        let blocks = blocks_of(buf.len())?;
        let multi = if blocks > 1 { TM_MULTI } else { 0 };
        self.issue(&cmd, TM_READ | TM_BLOCK_COUNT | multi, true, blocks)?;
        let r1 = self.response_r1(cmd.index)?;
        let deadline = self.data_deadline(blocks, false);
        let total = buf.len() / SECTOR;
        for (i, block) in buf.chunks_exact_mut(SECTOR).enumerate() {
            self.next_block(INT_BUF_READ, deadline, cmd.index, total - i)?;
            for word in block.chunks_exact_mut(4) {
                word.copy_from_slice(&self.bus.read(DATA_PORT).to_le_bytes());
            }
        }
        self.wait(INT_XFER_COMPLETE, Stage::Data, deadline, cmd.index)?;
        self.residual()?;
        Ok(r1)
    }

    /// A write command whose data moves by PIO from `buf` (whole blocks); returns the R1.
    /// Transfer complete follows the card's busy, so the blocks are programmed when this
    /// returns (the boot loader's path: it writes the BSB before it loads).
    pub(crate) fn write_pio(&mut self, cmd: Command, buf: &[u8]) -> Result<u32, Error> {
        let result = self.write_pio_inner(cmd, buf);
        if result.is_err() {
            let _ = self.reset_lines(RESET_CMD | RESET_DATA);
        }
        result
    }

    fn write_pio_inner(&mut self, cmd: Command, buf: &[u8]) -> Result<u32, Error> {
        let blocks = blocks_of(buf.len())?;
        let multi = if blocks > 1 { TM_MULTI } else { 0 };
        self.issue(&cmd, TM_BLOCK_COUNT | multi, true, blocks)?;
        let r1 = self.response_r1(cmd.index)?;
        let deadline = self.data_deadline(blocks, true);
        let total = buf.len() / SECTOR;
        for (i, block) in buf.chunks_exact(SECTOR).enumerate() {
            self.next_block(INT_BUF_WRITE, deadline, cmd.index, total - i)?;
            for word in block.chunks_exact(4) {
                self.bus.write(DATA_PORT, u32::from_le_bytes([word[0], word[1], word[2], word[3]]));
            }
        }
        self.wait(INT_XFER_COMPLETE, Stage::Data, deadline, cmd.index)?;
        self.residual()?;
        Ok(r1)
    }

    /// A data command whose `blocks` move by ADMA2 through the descriptor table at bus
    /// address `table` (written and handed to the device by the caller); returns the R1.
    pub(crate) fn adma(
        &mut self,
        cmd: Command,
        write: bool,
        blocks: u16,
        table: u32,
    ) -> Result<u32, Error> {
        if self.caps & CAP_ADMA2 == 0 {
            return Err(Error::Unsupported);
        }
        let result = self.adma_inner(cmd, write, blocks, table);
        if result.is_err() {
            let _ = self.reset_lines(RESET_CMD | RESET_DATA);
        }
        result
    }

    fn adma_inner(
        &mut self,
        cmd: Command,
        write: bool,
        blocks: u16,
        table: u32,
    ) -> Result<u32, Error> {
        rmw(&self.bus, HOST_CTRL, HC1_DMA_MASK, HC1_ADMA2_32);
        self.bus.write(ADMA_ADDR, table);
        self.bus.write(ADMA_ADDR_HI, 0);
        let dir = if write { 0 } else { TM_READ };
        let multi = if blocks > 1 { TM_MULTI } else { 0 };
        self.issue(&cmd, TM_DMA | TM_BLOCK_COUNT | dir | multi, true, blocks)?;
        let r1 = self.response_r1(cmd.index)?;
        let deadline = self.data_deadline(blocks, write);
        self.wait(INT_XFER_COMPLETE, Stage::Data, deadline, cmd.index)?;
        self.residual()?;
        Ok(r1)
    }

    /// Wait for the lines, program the block word and the argument, issue.
    fn issue(&mut self, cmd: &Command, mode: u32, data: bool, blocks: u16) -> Result<(), Error> {
        let busy = data || cmd.resp == Resp::R1b;
        let inhibit = PS_CMD_INHIBIT | if busy { PS_DAT_INHIBIT } else { 0 };
        poll(&self.bus, &mut self.platform, Stage::Inhibit, INHIBIT_TIMEOUT_US, POLL_US, |b| {
            b.read(PRESENT) & inhibit == 0
        })?;
        if data {
            self.bus.write(BLOCK, block_word(blocks));
        }
        self.bus.write(ARGUMENT, cmd.arg);
        self.bus.write(XFER_CMD, cmd.word(mode, data));
        Ok(())
    }

    /// Wait for the command's completion and return its R1.
    fn response_r1(&mut self, cmd: u8) -> Result<u32, Error> {
        let deadline = self.platform.now_us().saturating_add(COMMAND_TIMEOUT_US);
        self.wait(INT_CMD_COMPLETE, Stage::Command, deadline, cmd)?;
        Ok(self.bus.read(RESPONSE))
    }

    fn response(&self, resp: Resp) -> [u32; 4] {
        match resp {
            Resp::None => [0; 4],
            Resp::R2 => [0, 4, 8, 12].map(|at| self.bus.read(RESPONSE + at)),
            Resp::R1 | Resp::R1b | Resp::R3 => [self.bus.read(RESPONSE), 0, 0, 0],
        }
    }

    fn data_deadline(&self, blocks: u16, write: bool) -> u64 {
        let base = if write { WRITE_TIMEOUT_US } else { READ_TIMEOUT_US };
        let limit = base + u64::from(blocks) * PER_BLOCK_US;
        self.platform.now_us().saturating_add(limit)
    }

    /// A transfer that completed with blocks left is short.
    fn residual(&self) -> Result<(), Error> {
        match (self.bus.read(BLOCK) >> 16) as u16 {
            0 => Ok(()),
            remaining => Err(Error::ShortTransfer { remaining }),
        }
    }

    /// Wait until one of `want` latches — an error bit first — and clear what was taken.
    fn wait(&mut self, want: u32, stage: Stage, deadline: u64, cmd: u8) -> Result<(), Error> {
        self.wait_for(want, stage, deadline, cmd).map(|_| ())
    }

    /// The next block of a PIO transfer: the buffer is ready (`true`), or the controller ended
    /// the transfer before it (`false` — it completed instead of asking for the block).
    fn next_block(
        &mut self,
        buffer: u32,
        deadline: u64,
        cmd: u8,
        left: usize,
    ) -> Result<(), Error> {
        let fired = self.wait_for(buffer | INT_XFER_COMPLETE, Stage::Data, deadline, cmd)?;
        if fired & INT_XFER_COMPLETE == 0 {
            return Ok(());
        }
        // Ended early: what the controller counts as left is short; if it counts nothing, the
        // blocks this side still holds are.
        self.residual()?;
        Err(Error::ShortTransfer { remaining: u16::try_from(left).unwrap_or(u16::MAX) })
    }

    /// Waits for any bit of `want`; returns the ones that fired (cleared).
    fn wait_for(&mut self, want: u32, stage: Stage, deadline: u64, cmd: u8) -> Result<u32, Error> {
        loop {
            let status = self.bus.read(INT_STATUS);
            if status & ERR_ALL != 0 {
                self.bus.write(INT_STATUS, status & INT_USED);
                return Err(if status & ERR_ADMA != 0 {
                    Error::Adma { cmd, status: self.bus.read(ADMA_ERROR) & 0xFF }
                } else {
                    Error::Controller { cmd, status: status & ERR_ALL }
                });
            }
            if status & want != 0 {
                self.bus.write(INT_STATUS, status & want);
                return Ok(status & want);
            }
            if self.platform.now_us() >= deadline {
                return Err(Error::Timeout(stage));
            }
            self.platform.wait_irq(deadline);
        }
    }
}

/// Whole blocks, at least one, at most what one transfer counts.
pub(crate) fn blocks_of(len: usize) -> Result<u16, Error> {
    if len == 0 || len % SECTOR != 0 {
        return Err(Error::Range);
    }
    u16::try_from(len / SECTOR).map_err(|_| Error::Range)
}
