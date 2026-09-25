// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: init's reading of the boot disk (RFC-0098 C5, ADR-0067, TASK-0246 P4b). The loader
//! records the medium the boot came from in `/chosen/nexus,boot-disk`; init resolves the
//! record (`storage::boot_disk`) to the ONE device the block owner is granted — a virtio
//! transport the probe found carrying a block device, the K1's SD host node, or an SD host
//! controller the PCI plan placed at that function — asks policyd for the kind's class
//! (`device.mmio.blk` / `device.mmio.mmc`), and gives a PCI function bus mastering after its
//! grant, that function alone. Without a record (a direct-kernel dev boot, ADR-0059) the
//! lowest virtio block device is granted and the marker says there was no record; a record
//! that does not resolve is refused by name and nothing is granted. One line either way.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the record is host-tested in `storage` (`tests/boot_disk.rs`); this glue by
//!   every QEMU boot (`init: boot disk ok (` is required wherever blkd runs)

use core::fmt::Write as _;

use nexus_pci::{Bdf, PciHost};
use storage::boot_disk::{self, Kind, Place, CHOSEN_KEY};

use crate::bootstrap::device_tree::{self, DeviceWindow, VirtioDevices};
use crate::bootstrap::diag::Line;
use crate::bootstrap::pci::{PciDevices, SdHost};

/// The disk the block owner is granted.
#[derive(Clone, Copy)]
pub(crate) struct BootDisk {
    /// The window the grant carries.
    pub window: DeviceWindow,
    /// What it is.
    pub kind: Kind,
    /// A function behind an ECAM host: bus mastering follows the grant.
    pub pci: Option<SdHost>,
    /// The loader recorded it (false: the direct-kernel fallback).
    pub recorded: bool,
}

/// The loader's record resolved to the grant, or why nothing may be granted.
pub(crate) fn resolve(devices: &VirtioDevices, pci: &PciDevices) -> Result<BootDisk, &'static str> {
    let fdt = device_tree::tree().ok_or("no-device-tree")?;
    let Some(disk) = boot_disk::resolve(&fdt).map_err(|reject| reject.name())? else {
        // ADR-0059: a direct-kernel dev boot has no loader, so no record; the virtio disk the
        // probe found first stands in, and the marker says there was no record.
        let window = devices.blk[0].ok_or("no-virtio-block-device")?;
        return Ok(BootDisk { window, kind: Kind::VirtioBlk, pci: None, recorded: false });
    };
    match disk.place {
        Place::Node(node) => {
            let window = device_tree::window_of(node).ok_or("no-window")?;
            // A virtio transport is a disk only if the probe read device id 2 through it.
            let probed = devices.blk.iter().flatten().any(|w| w.base == window.base);
            if disk.kind == Kind::VirtioBlk && !probed {
                return Err("not-a-virtio-block-device");
            }
            Ok(BootDisk { window, kind: disk.kind, pci: None, recorded: true })
        }
        Place::Pci { host, dev, func } => {
            let host = PciHost::from_node(&fdt, host).map_err(|_| "malformed-ecam-host")?;
            let (ecam, _) = crate::bootstrap::pci::ecam_of(&host)?;
            let bdf = Bdf { bus: host.first_bus, dev, func };
            // The plan placed it and it is an SD host (class 0805): the record's generic name
            // and the function's class agree, or nothing is granted.
            let sd = pci.sd_host(ecam, bdf).ok_or("no-sd-host-at-that-function")?;
            Ok(BootDisk { window: sd.window, kind: disk.kind, pci: Some(sd), recorded: true })
        }
    }
}

/// The disk marker: the record (or its absence), the kind, the line, and — for a function
/// behind PCI — that bus mastering is on.
pub(crate) fn report(disk: &BootDisk) {
    let mut line = Line::new();
    let record = device_tree::chosen_str(CHOSEN_KEY).filter(|_| disk.recorded);
    let _ = write!(line, "init: boot disk ok (kind={} record=", disk.kind.name());
    let _ = match record {
        Some(path) => line.write_str(path),
        None => write!(line, "none window=0x{:x}", disk.window.base),
    };
    let _ = write!(line, " irq={}", disk.window.irq);
    if disk.pci.is_some() {
        let _ = line.write_str(" master=on");
    }
    let _ = line.write_str(")");
    line.emit();
}

/// A record that names no disk init may grant.
pub(crate) fn refused(reason: &str) {
    let mut line = Line::new();
    let _ = write!(line, "init: boot disk FAIL (reason={reason})");
    line.emit();
}
