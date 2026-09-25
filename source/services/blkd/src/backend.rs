// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Which backend the block owner runs (ADR-0067, TASK-0246 P4b). The loader's
//! boot-disk record in the tree names the disk and its kind (`storage::boot_disk`); the window
//! init granted must be that disk's — a node's first `reg`, or a memory window of the ECAM host
//! above a function — so the owner never drives a device the record does not name. A virtio
//! transport runs the virtio-blk backend; an SD host runs the SDHCI core with the
//! configuration its node gives (`storage::sdhci::host_config`). Without a record (a
//! direct-kernel dev boot, where init granted the lowest virtio disk) the disk node at the
//! granted window stands in. Pure: the tree and the window are the only inputs.
//! OWNERS: @runtime

use nexus_fdt::Fdt;
use nexus_pci::{PciHost, WindowKind};
use storage::boot_disk::{self, BootDisk, Kind, Place, Reject};
use storage_sdhci::HostConfig;

/// The backend a disk needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The virtio-blk driver over the transport.
    VirtioBlk,
    /// The SDHCI core, configured as the tree says.
    Sdhci(HostConfig),
}

/// The disk the owner serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selected {
    /// What it is.
    pub kind: Kind,
    /// What drives it.
    pub backend: Backend,
    /// The loader recorded it (false: the direct-kernel stand-in).
    pub recorded: bool,
}

/// Why the owner refuses the device it was granted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The record does not resolve.
    Record(Reject),
    /// The granted window is not the recorded disk's.
    WindowMismatch,
    /// No record, and no disk node at the granted window.
    NoDisk,
    /// The SD host's node configures no host (a bus width no bus has).
    HostConfig,
}

impl Refusal {
    /// The name markers use.
    pub fn name(self) -> &'static str {
        match self {
            Refusal::Record(reject) => reject.name(),
            Refusal::WindowMismatch => "window-mismatch",
            Refusal::NoDisk => "no-disk-at-window",
            Refusal::HostConfig => "host-config",
        }
    }
}

/// The backend for the device granted at `window` (a CPU address).
pub fn select(fdt: &Fdt<'_>, window: u64) -> Result<Selected, Refusal> {
    let (disk, recorded) = match boot_disk::resolve(fdt).map_err(Refusal::Record)? {
        Some(disk) if granted(fdt, &disk, window) => (disk, true),
        Some(_) => return Err(Refusal::WindowMismatch),
        None => (boot_disk::node_at_window(fdt, window).ok_or(Refusal::NoDisk)?, false),
    };
    let backend = match disk.kind {
        Kind::VirtioBlk => Backend::VirtioBlk,
        Kind::SdhciK1 | Kind::SdhciPci => {
            Backend::Sdhci(storage::sdhci::host_config(&disk).ok_or(Refusal::HostConfig)?)
        }
    };
    Ok(Selected { kind: disk.kind, backend, recorded })
}

/// The granted window is the disk's: a node's first register window, or inside a memory
/// window of the ECAM host above a function (the plan placed its BAR there).
fn granted(fdt: &Fdt<'_>, disk: &BootDisk<'_>, window: u64) -> bool {
    match disk.place {
        Place::Node(node) => node.reg(0).ok().flatten().is_some_and(|reg| reg.addr == window),
        Place::Pci { host, .. } => PciHost::from_node(fdt, host).is_ok_and(|host| {
            host.windows()
                .iter()
                .any(|w| w.kind != WindowKind::Io && window >= w.cpu && window - w.cpu < w.size)
        }),
    }
}
