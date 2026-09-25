// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]

//! CONTEXT: blkd's disk (ADR-0067, TASK-0246 P4b): the device init granted before this task
//! ran — in place, or not coming, so nothing here waits for it — driven by the backend its kind
//! needs. The loader's boot-disk record in the tree names the disk and the granted window must
//! be that disk's (`blkd::backend::select`); socd brings the disk's node up before the
//! controller is touched and, on the K1, names its `io` clock's rate (TASK-0246 P4c); a virtio
//! transport runs the virtio-blk driver, an SD host the SDHCI core (`storage::sdhci`). One line
//! says what runs, or why nothing does.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the selection is host-tested (`tests/backend.rs`); QEMU: `blkd: backend ok (`
//!   in every profile that runs the block plane

extern crate alloc;

use alloc::boxed::Box;
use core::fmt::Write as _;

use blkd::backend::{self, Backend};
use nexus_service_topology::slots::blkd::{DEVICE_TREE, IRQ_NOTIFY, REPLY, SOCD, WATCHDOG};
use nexus_wire::soc;
use storage::boot_disk::{Kind, CHOSEN_KEY};
use storage::sdhci::OsSdhciDevice;
use storage::virtio_blk::VirtioBlkDevice;
use storage::{BlockDevice, BlockError};
use storage_sdhci::{Layer, Mode};

/// The slot the granted device's capability lands in.
const MMIO_CAP_SLOT: u32 = nexus_service_topology::DEVICE_MMIO_SLOT;
/// The owner's two socd requests, one each: the reply inbox is its own, so a fixed nonce names
/// each answer.
const NONCE_BRING_UP: u32 = 1;
const NONCE_CLOCK_RATE: u32 = 2;

/// The disk, driven by the backend its kind needs. Either device lives on the heap, allocated
/// once at attach (the SDHCI one carries the run tables of its DMA buffers).
pub(crate) enum Disk {
    Virtio(Box<VirtioBlkDevice>),
    Sdhci { dev: Box<OsSdhciDevice>, irq_bound: bool },
}

impl Disk {
    /// Completions wake the owner through its notify endpoint: virtio binds the line now; the
    /// SDHCI core bound it before the card came up (its initialisation waits on it).
    pub(crate) fn bind_irq(&mut self) -> bool {
        match self {
            Disk::Virtio(dev) => dev.bind_irq_endpoint(IRQ_NOTIFY),
            Disk::Sdhci { irq_bound, .. } => *irq_bound,
        }
    }
}

impl BlockDevice for Disk {
    fn block_size(&self) -> usize {
        match self {
            Disk::Virtio(dev) => dev.block_size(),
            Disk::Sdhci { dev, .. } => dev.block_size(),
        }
    }

    fn block_count(&self) -> u64 {
        match self {
            Disk::Virtio(dev) => dev.block_count(),
            Disk::Sdhci { dev, .. } => dev.block_count(),
        }
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        match self {
            Disk::Virtio(dev) => dev.read_block(block_idx, buf),
            Disk::Sdhci { dev, .. } => dev.read_block(block_idx, buf),
        }
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        match self {
            Disk::Virtio(dev) => dev.write_block(block_idx, buf),
            Disk::Sdhci { dev, .. } => dev.write_block(block_idx, buf),
        }
    }

    fn read_blocks(&self, first_block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        match self {
            Disk::Virtio(dev) => dev.read_blocks(first_block, buf),
            Disk::Sdhci { dev, .. } => dev.read_blocks(first_block, buf),
        }
    }

    fn write_blocks(&mut self, first_block: u64, buf: &[u8]) -> Result<(), BlockError> {
        match self {
            Disk::Virtio(dev) => dev.write_blocks(first_block, buf),
            Disk::Sdhci { dev, .. } => dev.write_blocks(first_block, buf),
        }
    }

    fn sync(&mut self) -> Result<(), BlockError> {
        match self {
            Disk::Virtio(dev) => dev.sync(),
            Disk::Sdhci { dev, .. } => dev.sync(),
        }
    }
}

/// The disk init granted, opened by its backend; `None` — after one line saying why — when
/// nothing was granted or the grant is refused.
pub(crate) fn open() -> Option<Disk> {
    let mut query = nexus_abi::CapQuery::default();
    if nexus_abi::cap_query(MMIO_CAP_SLOT, &mut query).is_err() || query.kind_tag != 2 {
        refused(format_args!("no-disk-granted"));
        return None;
    }
    let tree = nexus_abi::device_tree::map_read_only(DEVICE_TREE)
        .and_then(|bytes| nexus_fdt::Fdt::new(bytes).ok());
    let Some(tree) = tree else {
        refused(format_args!("no-device-tree"));
        return None;
    };
    let selected = match backend::select(&tree, query.base) {
        Ok(selected) => selected,
        Err(refusal) => {
            refused(format_args!("{}", refusal.name()));
            return None;
        }
    };
    let record =
        tree.chosen().ok().and_then(|c| c.nexus_str(CHOSEN_KEY)).filter(|_| selected.recorded);
    // RFC-0106: the disk's node up before the controller is touched — power domains, resets,
    // clocks, pads; `not-needed` when the node names none (QEMU virt). A function behind PCI
    // has no node of its own.
    let soc = match selected.node {
        Some(node) => soc_bring_up(node.as_str())?,
        None => "none",
    };
    let facts = Facts { kind: selected.kind, soc, record };
    match selected.backend {
        Backend::VirtioBlk => match VirtioBlkDevice::new(MMIO_CAP_SLOT, WATCHDOG) {
            Ok(dev) => {
                report(&facts, dev.block_count(), None);
                Some(Disk::Virtio(Box::new(dev)))
            }
            Err(_) => {
                refused(format_args!("device-open"));
                None
            }
        },
        Backend::Sdhci(mut config) => {
            if config.layer == Layer::K1 {
                // The K1's capability register names no base clock: its `io` clock's rate.
                config.base_clock_hz = Some(soc_io_clock(selected.node.as_ref())?);
            }
            match storage::sdhci::open(MMIO_CAP_SLOT, IRQ_NOTIFY, WATCHDOG, config) {
                Ok((dev, opened)) => {
                    let mode = dev.disk().map(|disk| disk.card().mode());
                    report(&facts, dev.block_count(), Some((mode, opened.fallback)));
                    Some(Disk::Sdhci { dev: Box::new(dev), irq_bound: opened.irq_bound })
                }
                Err(error) => {
                    refused(format_args!("sdhci-open error={error:?}"));
                    None
                }
            }
        }
    }
}

/// socd's verdict on the disk's node — `ok` or `not-needed`; anything else refuses the disk
/// (one line says why).
fn soc_bring_up(path: &str) -> Option<&'static str> {
    match nexus_ipc::socd::bring_up(SOCD.send, REPLY, path, NONCE_BRING_UP) {
        Ok(reply) if reply.status == soc::STATUS_OK => Some("ok"),
        Ok(reply) if reply.status == soc::STATUS_NOT_NEEDED => Some("not-needed"),
        Ok(reply) => {
            refused(format_args!("soc-bring-up status={}", reply.status));
            None
        }
        Err(error) => {
            refused(format_args!("soc-bring-up error={error:?}"));
            None
        }
    }
}

/// The rate of the node's `io` clock, from socd; none refuses the disk (one line says why).
fn soc_io_clock(node: Option<&backend::NodePath>) -> Option<u32> {
    let Some(node) = node else {
        refused(format_args!("soc-clock no-node"));
        return None;
    };
    match nexus_ipc::socd::clock_rate(SOCD.send, REPLY, node.as_str(), "io", NONCE_CLOCK_RATE) {
        Ok((soc::STATUS_OK, hz)) if hz > 0 => match u32::try_from(hz) {
            Ok(hz) => Some(hz),
            Err(_) => {
                refused(format_args!("soc-clock hz={hz}"));
                None
            }
        },
        Ok((status, _)) => {
            refused(format_args!("soc-clock status={status}"));
            None
        }
        Err(error) => {
            refused(format_args!("soc-clock error={error:?}"));
            None
        }
    }
}

/// What the marker says about every disk.
struct Facts<'a> {
    kind: Kind,
    soc: &'static str,
    record: Option<&'a str>,
}

/// `blkd: backend ok (kind=… soc=… record=… [mode=… bus=…] sectors=… [fallback=…])`.
fn report(
    facts: &Facts<'_>,
    sectors: u64,
    sdhci: Option<(Option<Mode>, Option<storage_sdhci::Error>)>,
) {
    let mut line = Line::new();
    let _ = write!(
        line,
        "blkd: backend ok (kind={} soc={} record={}",
        facts.kind.name(),
        facts.soc,
        facts.record.unwrap_or("none")
    );
    if let Some((mode, fallback)) = sdhci {
        let (name, bus) = match mode {
            Some(Mode::Legacy { width }) => ("legacy", width),
            Some(Mode::Hs52 { width }) => ("hs52", width),
            Some(Mode::Hs400es) => ("hs400es", 8),
            None => ("unknown", 0),
        };
        let _ = write!(line, " mode={name} bus={bus}");
        if let Some(reason) = fallback {
            let _ = write!(line, " fallback={reason:?}");
        }
    }
    let _ = write!(line, " sectors={sectors})");
    line.emit();
}

/// `blkd: backend FAIL (reason=…)`.
fn refused(reason: core::fmt::Arguments<'_>) {
    let mut line = Line::new();
    let _ = write!(line, "blkd: backend FAIL (reason={reason})");
    line.emit();
}

/// One line, formatted on the stack and printed with one call.
struct Line {
    buf: [u8; 192],
    len: usize,
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let take = s.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}

impl Line {
    fn new() -> Self {
        Self { buf: [0; 192], len: 0 }
    }

    fn emit(self) {
        if let Ok(text) = core::str::from_utf8(&self.buf[..self.len]) {
            let _ = nexus_abi::debug_println(text);
        }
    }
}
