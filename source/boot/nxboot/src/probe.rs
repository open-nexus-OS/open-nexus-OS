// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The boot disk on the target (TASK-0246B P1/P2, RFC-0098 C5). The candidates the tree
//! lists, in the rule's order — virtio block transports by address; SD hosts that may hold an
//! eMMC by address (`nxboot::disk::node::emmc_hosts`: not marked `no-mmc`); SD host controllers
//! behind each ECAM host, placed by the planner init runs too (QEMU's `sdhci-pci`: nothing
//! assigns its BAR before this) — go to `nxboot::disk::pick`, which takes the first that opens
//! and carries a valid BSB. A host in the tree opens once its glue is up and its `io` clock read
//! (`nxboot::disk::node::open_config`, over the provider windows at their physical addresses —
//! only for the candidate being opened). Each one skipped gets a line, a glue fault one more
//! with the register and what it read; the chosen one is named by its record
//! (`/chosen/nexus,boot-disk`: the node path, or `<ECAM host>/mmc@<dev>,<func>`). The SDHCI
//! reader runs over physical registers and `rdtime` (no interrupt, no DMA).
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the rule and the SDHCI reader are host-tested (`tests/loader_flow.rs`); this
//!   glue by every QEMU boot (`nxboot: fdt ok (… disk=…)`) and the SDHCI lane (TASK-0246 P5)

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use nexus_fdt::{Fdt, Node};
use nexus_hal::Bus;
use nexus_pci::{Ecam, PciHost};
use nexus_soc::Fault;
use nxboot::disk::node::{self, Refusal};
use nxboot::disk::{self, sdhci::SdhciDisk, Skip};
use storage::boot_disk::{self as record, Kind, Place, CLASS_SD_HOST, MAX_RECORD};
use storage::{BlockDevice, BlockError};
use storage_sdhci::{Error, HostConfig, Platform};

use crate::arch;
use crate::platform::Tree;
use crate::virtio::VirtioDisk;

/// Between two reads of a status that raises no interrupt the loader could take.
const POLL_US: u64 = 10;

/// Physical registers at `base` (the loader runs untranslated).
#[derive(Clone, Copy)]
pub struct ArchBus {
    base: usize,
}

impl ArchBus {
    /// The bus at physical addresses, as the loader runs.
    pub const UNTRANSLATED: ArchBus = ArchBus { base: 0 };
}

impl Bus for ArchBus {
    fn read(&self, addr: usize) -> u32 {
        arch::mmio_read32(self.base + addr)
    }

    fn write(&self, addr: usize, value: u32) {
        arch::mmio_write32(self.base + addr, value)
    }
}

/// Time from `rdtime` at the tree's timebase; no interrupt, so every wait polls, bounded by the
/// core's deadlines.
pub struct ArchPlatform {
    tb_hz: u64,
}

impl Platform for ArchPlatform {
    fn now_us(&self) -> u64 {
        let us = u128::from(arch::time_ticks()) * 1_000_000 / u128::from(self.tb_hz.max(1));
        u64::try_from(us).unwrap_or(u64::MAX)
    }

    fn delay_us(&mut self, us: u64) {
        let end = self.now_us().saturating_add(us);
        while self.now_us() < end {
            core::hint::spin_loop();
        }
    }

    fn wait_irq(&mut self, deadline_us: u64) {
        let now = self.now_us();
        if deadline_us > now {
            self.delay_us((deadline_us - now).min(POLL_US));
        }
    }

    fn irq(&self) -> bool {
        false
    }
}

/// A place the boot volume may be.
enum Candidate {
    Virtio { node: Node<'static>, base: usize },
    SdNode { node: Node<'static>, base: usize },
    SdPci { host: Node<'static>, dev: u8, func: u8, bar: usize },
}

/// The disk the loader boots from.
pub enum BootDisk {
    Virtio(VirtioDisk),
    Sdhci(SdhciDisk<ArchBus, ArchPlatform>),
}

impl BlockDevice for BootDisk {
    fn block_size(&self) -> usize {
        match self {
            BootDisk::Virtio(d) => d.block_size(),
            BootDisk::Sdhci(d) => d.block_size(),
        }
    }

    fn block_count(&self) -> u64 {
        match self {
            BootDisk::Virtio(d) => d.block_count(),
            BootDisk::Sdhci(d) => d.block_count(),
        }
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        match self {
            BootDisk::Virtio(d) => d.read_block(block_idx, buf),
            BootDisk::Sdhci(d) => d.read_block(block_idx, buf),
        }
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        match self {
            BootDisk::Virtio(d) => d.write_block(block_idx, buf),
            BootDisk::Sdhci(d) => d.write_block(block_idx, buf),
        }
    }

    fn read_blocks(&self, first_block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        match self {
            BootDisk::Virtio(d) => d.read_blocks(first_block, buf),
            BootDisk::Sdhci(d) => d.read_blocks(first_block, buf),
        }
    }

    fn write_blocks(&mut self, first_block: u64, buf: &[u8]) -> Result<(), BlockError> {
        match self {
            BootDisk::Virtio(d) => d.write_blocks(first_block, buf),
            BootDisk::Sdhci(d) => d.write_blocks(first_block, buf),
        }
    }

    fn sync(&mut self) -> Result<(), BlockError> {
        match self {
            BootDisk::Virtio(d) => d.sync(),
            BootDisk::Sdhci(d) => d.sync(),
        }
    }
}

/// The boot disk and the record that names it (`record[..len]`); `None` when no candidate
/// carries a valid BSB.
pub fn find(tree: &Tree) -> Option<(BootDisk, [u8; MAX_RECORD], usize)> {
    let tb_hz = tree.fdt.cpus().map(|c| u64::from(c.timebase_hz)).unwrap_or(0);
    let (chosen, disk) = disk::pick(
        candidates(&tree.fdt),
        |c| open(&tree.fdt, c, tb_hz),
        |c, why| {
            let mut buf = [0u8; MAX_RECORD];
            let name = record_of(c, &mut buf).unwrap_or("?");
            arch::uart_puts(&format!("nxboot: disk {name} skipped ({})\n", skip_str(why)));
        },
    )?;
    let mut rec = [0u8; MAX_RECORD];
    let len = record_of(&chosen, &mut rec)?.len();
    Some((disk, rec, len))
}

/// Every candidate the tree lists, in the rule's order.
fn candidates(fdt: &Fdt<'static>) -> Vec<Candidate> {
    let window = |node: &Node<'static>| {
        let reg = node.reg(0).ok().flatten()?;
        usize::try_from(reg.addr).ok()
    };
    let mut virtio: Vec<(usize, Node<'static>)> = fdt
        .find_compatible(&["virtio,mmio"])
        .filter(|n| n.is_enabled())
        .filter_map(|n| Some((window(&n)?, n)))
        .filter(|(base, _)| VirtioDisk::is_block(*base))
        .collect();
    virtio.sort_unstable_by_key(|(base, _)| *base);

    let mut out: Vec<Candidate> =
        virtio.into_iter().map(|(base, node)| Candidate::Virtio { node, base }).collect();
    out.extend(
        node::emmc_hosts(fdt).into_iter().map(|(base, node)| Candidate::SdNode { node, base }),
    );
    for host in fdt.find_compatible(&["pci-host-ecam-generic"]).filter(|n| n.is_enabled()) {
        pci_sd_hosts(fdt, host, &mut out);
    }
    out
}

/// The SD host controllers on `host`'s root bus, placed by the planner init runs (memory
/// decoding on, bus mastering off — the loader reads by PIO).
fn pci_sd_hosts(fdt: &Fdt<'static>, host: Node<'static>, out: &mut Vec<Candidate>) {
    let Ok(pci) = PciHost::from_node(fdt, host) else {
        arch::uart_puts(&format!("nxboot: pci host {} skipped (malformed)\n", host.name()));
        return;
    };
    let Ok(ecam) = usize::try_from(pci.root_config().0) else { return };
    let Ok(plan) = nexus_pci::plan(&pci, &Ecam::new(ArchBus { base: 0 }, ecam, pci.first_bus, 1))
    else {
        arch::uart_puts(&format!("nxboot: pci host {} skipped (plan)\n", host.name()));
        return;
    };
    for f in plan.of_class(CLASS_SD_HOST) {
        let Some(bar) = f.bars[0] else { continue };
        let Ok(bar) = usize::try_from(bar.cpu) else { continue };
        out.push(Candidate::SdPci { host, dev: f.bdf.dev, func: f.bdf.func, bar });
    }
}

fn open(fdt: &Fdt<'static>, candidate: &Candidate, tb_hz: u64) -> Result<BootDisk, &'static str> {
    let sd = |base: usize, config: HostConfig| {
        SdhciDisk::open(ArchBus { base }, ArchPlatform { tb_hz }, config)
            .map(BootDisk::Sdhci)
            .map_err(sd_error)
    };
    match candidate {
        Candidate::Virtio { base, .. } => VirtioDisk::open(*base).map(BootDisk::Virtio),
        Candidate::SdNode { node, base } => {
            // Its glue up and its `io` clock read — the provider windows at their physical
            // addresses, for this candidate only.
            let config = node::open_config(fdt, *node, &ArchBus { base: 0 }).map_err(|why| {
                glue_fault_line(node, why);
                why.name()
            })?;
            let disk = sd(*base, config)?;
            // TASK-0327B P4 H0a: the mode the loader reads the slot with, next to its verdict.
            if let BootDisk::Sdhci(d) = &disk {
                let (mode, width) = d.mode_name();
                arch::uart_puts(&format!("nxboot: disk sdhci mode={mode} bus={width}\n"));
            }
            Ok(disk)
        }
        Candidate::SdPci { host, dev, func, bar } => {
            let place = Place::Pci { host: *host, dev: *dev, func: *func };
            let disk = record::BootDisk { place, kind: Kind::SdhciPci };
            sd(*bar, storage::sdhci::host_config(&disk).ok_or("host config")?)
        }
    }
}

fn record_of<'b>(candidate: &Candidate, out: &'b mut [u8]) -> Option<&'b str> {
    match candidate {
        Candidate::Virtio { node, .. } | Candidate::SdNode { node, .. } => {
            record::record_for_node(node, out)
        }
        Candidate::SdPci { host, dev, func, .. } => {
            record::record_for_pci_sd_host(host, *dev, *func, out)
        }
    }
}

/// The register and the value a failed glue step read, next to the skip line that names it —
/// the words of `socd`'s own failure marker (RFC-0106), so one search finds both stages.
fn glue_fault_line(node: &Node<'static>, why: Refusal) {
    let Refusal::Glue(Fault::ReadBack { addr, value } | Fault::FcStuck { addr, value }) = why
    else {
        return;
    };
    let mut buf = [0u8; MAX_RECORD];
    let path = record::record_for_node(node, &mut buf).unwrap_or("?");
    arch::uart_puts(&format!("nxboot: bring-up {path} FAIL (reg=0x{addr:x} val=0x{value:x})\n"));
}

fn sd_error(error: Error) -> &'static str {
    match error {
        Error::NoBaseClock => "no base clock",
        Error::Timeout(_) => "no answer",
        Error::Controller { .. } => "no card",
        Error::ByteAddressed => "byte-addressed card",
        Error::Voltage => "no shared voltage",
        _ => "card init",
    }
}

fn skip_str(why: Skip) -> &'static str {
    match why {
        Skip::NoDevice(reason) => reason,
        Skip::NoLayout => "no layout",
        Skip::NoValidBsb => "no valid bsb",
    }
}
