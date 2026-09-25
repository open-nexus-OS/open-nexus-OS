// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Sectors by ADMA2: an initialised [`Card`] plus two DMA buffers made for the controller —
//! the descriptor table (one run: its bus address is what the controller is given) and a
//! bounce buffer the data crosses. Every transfer takes both buffers through their
//! typestate: the descriptors are written while the CPU owns the table, the table goes to
//! the device cleaned, the bounce buffer goes cleaned (write) or flushed (read), and only
//! after the controller finished do they come back — so a non-coherent device (the K1)
//! never reads a stale line and the CPU never reads one. A request larger than the bounce
//! buffer moves in chunks, in order; a failed chunk ends the request with its error.
//! OWNERS: @runtime @drivers

use nexus_driverkit::{CacheOps, Direction, DmaBuffer, DmaMemory};
use nexus_hal::Bus;

use crate::adma::{self, DESC_BYTES};
use crate::engine::SECTOR;
use crate::{Card, Error, Platform};

/// An eMMC and the DMA memory its controller moves sectors through.
pub struct Disk<B: Bus, P: Platform, M: DmaMemory, C: CacheOps> {
    card: Card<B, P>,
    table: Option<DmaBuffer<M, C>>,
    bounce: Option<DmaBuffer<M, C>>,
    /// Sectors per chunk.
    chunk: u16,
}

impl<B: Bus, P: Platform, M: DmaMemory, C: CacheOps> Disk<B, P, M, C> {
    /// `table` must be one run below 4 GiB with room for the bounce buffer's descriptors;
    /// `bounce` whole sectors below 4 GiB. Both are checked by building the descriptors of a
    /// full chunk once.
    pub fn new(
        card: Card<B, P>,
        mut table: DmaBuffer<M, C>,
        bounce: DmaBuffer<M, C>,
    ) -> Result<Self, Error> {
        let runs = table.runs();
        if runs.len() != 1 {
            return Err(Error::DmaMemory(adma::AdmaError::TableSplit));
        }
        if runs[0].bus % 4 != 0 || table.len() < DESC_BYTES {
            return Err(Error::DmaMemory(adma::AdmaError::Unaligned));
        }
        if runs[0].bus + runs[0].len > 1 << 32 {
            return Err(Error::DmaMemory(adma::AdmaError::Above4G));
        }
        let sectors = (bounce.len() / SECTOR).min(usize::from(u16::MAX));
        if sectors == 0 {
            return Err(Error::Range);
        }
        adma::build(bounce.runs(), sectors * SECTOR, table.bytes_mut())
            .map_err(Error::DmaMemory)?;
        Ok(Self { card, table: Some(table), bounce: Some(bounce), chunk: sectors as u16 })
    }

    /// The card.
    pub fn card(&self) -> &Card<B, P> {
        &self.card
    }

    /// The capacity in 512-byte sectors.
    pub fn sectors(&self) -> u32 {
        self.card.sectors()
    }

    /// Read `buf.len() / 512` sectors from `lba`.
    pub fn read(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), Error> {
        self.check(lba, buf.len())?;
        let step = usize::from(self.chunk) * SECTOR;
        for (i, part) in buf.chunks_mut(step).enumerate() {
            let at = lba + (i * usize::from(self.chunk)) as u32;
            self.transfer(
                at,
                part.len(),
                false,
                |_| {},
                |bytes| part.copy_from_slice(&bytes[..part.len()]),
            )?;
        }
        Ok(())
    }

    /// Write `buf.len() / 512` sectors at `lba`; each chunk is programmed when this returns.
    pub fn write(&mut self, lba: u32, buf: &[u8]) -> Result<(), Error> {
        self.check(lba, buf.len())?;
        let step = usize::from(self.chunk) * SECTOR;
        for (i, part) in buf.chunks(step).enumerate() {
            let at = lba + (i * usize::from(self.chunk)) as u32;
            self.transfer(
                at,
                part.len(),
                true,
                |bytes| bytes[..part.len()].copy_from_slice(part),
                |_| {},
            )?;
        }
        Ok(())
    }

    fn check(&self, lba: u32, len: usize) -> Result<(), Error> {
        if len == 0 || len % SECTOR != 0 {
            return Err(Error::Range);
        }
        let blocks = u64::try_from(len / SECTOR).map_err(|_| Error::Range)?;
        if u64::from(lba) + blocks > u64::from(self.card.sectors()) {
            return Err(Error::Range);
        }
        Ok(())
    }

    /// One chunk: fill (write), descriptors, both buffers to the device, the command, both
    /// back, drain (read).
    fn transfer(
        &mut self,
        lba: u32,
        len: usize,
        write: bool,
        fill: impl FnOnce(&mut [u8]),
        drain: impl FnOnce(&[u8]),
    ) -> Result<(), Error> {
        let (Some(mut table), Some(mut bounce)) = (self.table.take(), self.bounce.take()) else {
            return Err(Error::DmaBuffer);
        };
        fill(bounce.bytes_mut());
        let built = adma::build(bounce.runs(), len, table.bytes_mut()).map_err(Error::DmaMemory);
        let table_bus = table.runs()[0].bus as u32;
        let dir = if write { Direction::ToDevice } else { Direction::FromDevice };
        let table_in = table.for_device(Direction::ToDevice);
        let bounce_in = bounce.for_device(dir);
        let result =
            built.and_then(|_| self.card.adma(lba, (len / SECTOR) as u16, write, table_bus));
        let (table, bounce) = (table_in.for_cpu(), bounce_in.for_cpu());
        if result.is_ok() {
            drain(bounce.bytes());
        }
        self.table = Some(table);
        self.bounce = Some(bounce);
        result
    }
}
