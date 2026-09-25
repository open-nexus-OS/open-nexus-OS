// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The loader's SDHCI reader (TASK-0246B P1): the SDHCI core (TASK-0246 P2) in PIO — no
//! DMA memory, no interrupt, the loader has neither — as a `BlockDevice` the boot decision runs
//! over, reading the volume and writing the BSB (the A/B trial decrements it before the load).
//! It initialises the card itself at a conservative operating point (HS52 on the widest bus the
//! host and board share; HS400 is the OS's) and relies on no predecessor's controller state, so
//! QEMU and the board take one path. Generic over the core's bus and platform: the host proves it
//! against `storage-sdhci-model`, the target runs it over physical MMIO and `rdtime`.
//! OWNERS: @runtime @reliability

use core::cell::RefCell;

use nexus_hal::Bus;
use storage::{BlockDevice, BlockError};
use storage_sdhci::{Card, Ceiling, Error, Host, HostConfig, Platform};

/// A sector.
pub const SECTOR: usize = 512;

/// An eMMC read and written by PIO.
pub struct SdhciDisk<B: Bus, P: Platform> {
    card: RefCell<Card<B, P>>,
    sectors: u64,
}

impl<B: Bus, P: Platform> SdhciDisk<B, P> {
    /// The card behind the host on `bus`, initialised from power-up.
    pub fn open(bus: B, platform: P, config: HostConfig) -> Result<Self, Error> {
        let host = Host::new(bus, platform, config)?;
        let card = Card::init(host, Ceiling::Hs52).map_err(|failure| failure.error)?;
        let sectors = u64::from(card.sectors());
        Ok(Self { card: RefCell::new(card), sectors })
    }

    fn run(
        &self,
        first: u64,
        op: impl FnOnce(&mut Card<B, P>, u32) -> Result<(), Error>,
    ) -> Result<(), BlockError> {
        let lba = u32::try_from(first).map_err(|_| BlockError::OutOfRange)?;
        let mut card = self.card.try_borrow_mut().map_err(|_| BlockError::IoError)?;
        op(&mut card, lba).map_err(|e| match e {
            Error::Range => BlockError::OutOfRange,
            _ => BlockError::IoError,
        })
    }
}

impl<B: Bus, P: Platform> BlockDevice for SdhciDisk<B, P> {
    fn block_size(&self) -> usize {
        SECTOR
    }

    fn block_count(&self) -> u64 {
        self.sectors
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        let buf = buf.get_mut(..SECTOR).ok_or(BlockError::IoError)?;
        self.read_blocks(block_idx, buf)
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        let buf = buf.get(..SECTOR).ok_or(BlockError::IoError)?;
        self.write_blocks(block_idx, buf)
    }

    fn read_blocks(&self, first_block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.run(first_block, |card, lba| card.read_pio(lba, buf))
    }

    fn write_blocks(&mut self, first_block: u64, buf: &[u8]) -> Result<(), BlockError> {
        self.run(first_block, |card, lba| card.write_pio(lba, buf))
    }

    /// A PIO write returns programmed, and the card's volatile cache is never on.
    fn sync(&mut self) -> Result<(), BlockError> {
        Ok(())
    }
}
