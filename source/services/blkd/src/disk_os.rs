// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]

//! CONTEXT: blkd's disk (ADR-0067, TASK-0246 P4b): the device init granted before this task
//! ran — in place, or not coming, so nothing here waits for it — driven by the backend its kind
//! needs. The loader's boot-disk record in the tree names the disk and the granted window must
//! be that disk's (`blkd::backend::select`); a virtio transport runs the virtio-blk driver, an
//! SD host the SDHCI core (`storage::sdhci`). One line says what runs, or why nothing does.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the selection is host-tested (`tests/backend.rs`); QEMU: `blkd: backend ok (`
//!   in every profile that runs the block plane

extern crate alloc;

use alloc::boxed::Box;
use core::fmt::Write as _;

use blkd::backend::{self, Backend};
use nexus_service_topology::slots::blkd::{DEVICE_TREE, IRQ_NOTIFY, WATCHDOG};
use storage::boot_disk::{Kind, CHOSEN_KEY};
use storage::sdhci::OsSdhciDevice;
use storage::virtio_blk::VirtioBlkDevice;
use storage::{BlockDevice, BlockError};
use storage_sdhci::Mode;

/// The slot the granted device's capability lands in.
const MMIO_CAP_SLOT: u32 = nexus_service_topology::DEVICE_MMIO_SLOT;

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
    match selected.backend {
        Backend::VirtioBlk => match VirtioBlkDevice::new(MMIO_CAP_SLOT, WATCHDOG) {
            Ok(dev) => {
                report(selected.kind, record, dev.block_count(), None);
                Some(Disk::Virtio(Box::new(dev)))
            }
            Err(_) => {
                refused(format_args!("device-open"));
                None
            }
        },
        Backend::Sdhci(config) => {
            match storage::sdhci::open(MMIO_CAP_SLOT, IRQ_NOTIFY, WATCHDOG, config) {
                Ok((dev, facts)) => {
                    let mode = dev.disk().map(|disk| disk.card().mode());
                    report(selected.kind, record, dev.block_count(), Some((mode, facts.fallback)));
                    Some(Disk::Sdhci { dev: Box::new(dev), irq_bound: facts.irq_bound })
                }
                Err(error) => {
                    refused(format_args!("sdhci-open error={error:?}"));
                    None
                }
            }
        }
    }
}

/// `blkd: backend ok (kind=… record=… [mode=… bus=…] sectors=… [fallback=…])`.
fn report(
    kind: Kind,
    record: Option<&str>,
    sectors: u64,
    sdhci: Option<(Option<Mode>, Option<storage_sdhci::Error>)>,
) {
    let mut line = Line::new();
    let _ =
        write!(line, "blkd: backend ok (kind={} record={}", kind.name(), record.unwrap_or("none"));
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
