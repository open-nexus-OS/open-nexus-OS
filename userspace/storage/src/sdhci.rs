// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The SDHCI backend's block face (TASK-0246 P4b, ADR-0067): an eMMC behind an SDHCI
//! host as the block owner's `BlockDevice`, and the host configuration the tree gives the boot
//! disk ([`host_config`]). 512-byte sectors, the card's 32-bit sector addresses; reads through
//! `&self` (the block plane's shape), writes programmed when they return — the core never
//! enables the card's volatile cache — so `sync` has nothing left to do. Generic over the
//! core's bus, platform and DMA memory: the host proves it against the behavioural machine
//! (`storage-sdhci-model`), the OS runs it over `storage_sdhci::os` ([`open`]).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/sdhci.rs` (the block face over the machine: runs, single sectors,
//!   the ranges, a device error and the recovery; the host configuration from the trees)

use core::cell::{Ref, RefCell};

use nexus_driverkit::{CacheOps, DmaMemory};
use nexus_hal::Bus;
use storage_sdhci::{Disk, Error, HostConfig, Layer, Platform};

use crate::boot_disk::{BootDisk, Kind, Place};
use crate::{BlockDevice, BlockError};

/// The card's sector.
pub const SECTOR: usize = 512;

/// What the tree says about the boot disk's SD host; `None` when the disk is not an SD host
/// or its node is malformed.
pub fn host_config(disk: &BootDisk<'_>) -> Option<HostConfig> {
    match (disk.kind, disk.place) {
        // A standard host behind PCI: its capability register says the rest (the widest bus
        // it has, its base clock).
        (Kind::SdhciPci, Place::Pci { .. }) => Some(HostConfig {
            base_clock_hz: None,
            bus_width: 8,
            hs400es: false,
            layer: Layer::Standard,
        }),
        (Kind::SdhciK1, Place::Node(node)) => {
            // The mmc-controller binding: `bus-width` defaults to 1.
            let bus_width = match node.prop_u32("bus-width") {
                None => 1,
                Some(width) => u8::try_from(width).ok()?,
            };
            let hs400es = node.prop("mmc-hs400-enhanced-strobe").is_some()
                && node.prop("mmc-hs400-1_8v").is_some();
            // The K1's capability register names no base clock: the `io` clock's rate comes
            // from the SoC glue — socd's answer to the block owner (TASK-0246 P4c), the
            // loader's own `nexus_soc::clock_rate` (TASK-0246B P2); without it the host
            // refuses to start.
            Some(HostConfig { base_clock_hz: None, bus_width, hs400es, layer: Layer::K1 })
        }
        _ => None,
    }
}

/// An eMMC as the block owner's device.
pub struct SdhciDevice<B: Bus, P: Platform, M: DmaMemory, C: CacheOps> {
    disk: RefCell<Disk<B, P, M, C>>,
    sectors: u64,
}

impl<B: Bus, P: Platform, M: DmaMemory, C: CacheOps> SdhciDevice<B, P, M, C> {
    /// The block face of an initialised disk.
    pub fn new(disk: Disk<B, P, M, C>) -> Self {
        let sectors = u64::from(disk.sectors());
        Self { disk: RefCell::new(disk), sectors }
    }

    /// The disk (its card's mode and bus for the owner's marker); `None` while a transfer
    /// holds it.
    pub fn disk(&self) -> Option<Ref<'_, Disk<B, P, M, C>>> {
        self.disk.try_borrow().ok()
    }

    /// One run handed to the core at the card's 32-bit address. The core refuses what the
    /// card cannot hold — an empty or partial sector, a run past the end — before a command
    /// is issued (`Error::Range`); what only this face sees is an address wider than 32 bits.
    fn run(
        &self,
        first: u64,
        op: impl FnOnce(&mut Disk<B, P, M, C>, u32) -> Result<(), Error>,
    ) -> Result<(), BlockError> {
        let lba = u32::try_from(first).map_err(|_| BlockError::OutOfRange)?;
        let mut disk = self.disk.try_borrow_mut().map_err(|_| BlockError::IoError)?;
        op(&mut disk, lba).map_err(|e| match e {
            Error::Range => BlockError::OutOfRange,
            _ => BlockError::IoError,
        })
    }
}

impl<B: Bus, P: Platform, M: DmaMemory, C: CacheOps> BlockDevice for SdhciDevice<B, P, M, C> {
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
        self.run(first_block, |disk, lba| disk.read(lba, buf))
    }

    fn write_blocks(&mut self, first_block: u64, buf: &[u8]) -> Result<(), BlockError> {
        self.run(first_block, |disk, lba| disk.write(lba, buf))
    }

    /// A write returns programmed and the card's volatile cache is off: nothing is pending.
    fn sync(&mut self) -> Result<(), BlockError> {
        Ok(())
    }
}

/// The block face on the OS.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub type OsSdhciDevice = SdhciDevice<
    nexus_driverkit::Mmio,
    storage_sdhci::os::OsPlatform,
    nexus_abi::DmaVmo,
    nexus_driverkit::Zicbom,
>;

/// The SD host behind `device` as the owner's block device (`storage_sdhci::os::open`): its
/// line bound to `irq_ep`, its waits bounded on `watchdog`, and what bringing it up found.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub fn open(
    device: u32,
    irq_ep: u32,
    watchdog: nexus_service_topology::SlotPair,
    config: HostConfig,
) -> Result<(OsSdhciDevice, storage_sdhci::os::Facts), storage_sdhci::os::OpenError> {
    let (disk, facts) = storage_sdhci::os::open(device, irq_ep, watchdog, config)?;
    Ok((SdhciDevice::new(disk), facts))
}
