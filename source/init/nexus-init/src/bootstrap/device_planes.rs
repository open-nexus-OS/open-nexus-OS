// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The device planes' grants (RFC-0098 C3): every window and interrupt line init read
//! from the device tree — or from a PCI function the ECAM plan placed — goes to the one service
//! that owns it, policy-checked; a plane the machine lacks is named absent and its owner runs
//! without a window (the board has no virtio-net, virtio-rng or virtio-gpu; TASK-0260B P3:
//! `init` died on the first of them). The ONE disk (ADR-0044: blk[0]) went in the core plane; a
//! second blk device is nobody's — `/data` is a partition on the one disk. Split out of the
//! orchestrator under the module-size ratchet when the USB plane joined (TASK-0328 U1).
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Internal
//! TEST_COVERAGE: every QEMU lane (`init: <plane> none`, the owners' markers) and the board lanes

use crate::bootstrap::core_plane::{
    grant_display_plane, grant_mmio_with_wait, GrantStats, UsbPlane,
};
use crate::bootstrap::device_tree::VirtioDevices;
use crate::bootstrap::diag::iw;
use crate::bootstrap::route_provision::grant_rtc_mmio_to_timed;
use crate::os_payload::*;
use crate::service_topology::ServiceId;

/// The services the device planes go to.
pub(crate) struct PlaneOwners {
    pub netstackd: u32,
    pub rngd: u32,
    pub timed: u32,
    pub gpud: u32,
    pub hidrawd: u32,
    pub xhcid: u32,
    pub selftest: u32,
}

/// Every device plane to its owner, in the order the orchestrator granted them.
pub(crate) fn grant_device_planes(
    stats: &GrantStats,
    pol_route: (u32, u32),
    o: &PlaneOwners,
    devices: &VirtioDevices,
    usb: Option<UsbPlane>,
    (init_wire, init_fold): (&mut nexus_event::SpanTally, bool),
) -> Result<()> {
    match devices.net {
        Some(net) => grant_mmio_with_wait(
            stats,
            pol_route,
            o.netstackd,
            "netstackd",
            "device.mmio.net",
            net,
            DEVICE_MMIO_CAP_SLOT,
        )?,
        None => debug_write_bytes(b"init: net plane none (no device in the tree)\n"),
    }
    match devices.rng {
        Some(rng) => grant_mmio_with_wait(
            stats,
            pol_route,
            o.rngd,
            "rngd",
            "device.mmio.rng",
            rng,
            DEVICE_MMIO_CAP_SLOT,
        )?,
        None => debug_write_bytes(b"init: rng plane none (no device in the tree)\n"),
    }
    grant_rtc_mmio_to_timed(o.timed, pol_route.0, pol_route.1)?;
    match devices.gpu {
        Some(gpu) => grant_mmio_with_wait(
            stats,
            pol_route,
            o.gpud,
            "gpud",
            "device.mmio.gpu",
            gpu,
            DEVICE_MMIO_CAP_SLOT,
        )?,
        None => debug_write_bytes(b"init: gpu plane none (no device in the tree)\n"),
    }
    grant_display_plane(stats, pol_route, o.gpud)?;
    if let Some(net) = devices.net {
        grant_mmio_with_wait(
            stats,
            pol_route,
            o.selftest,
            "selftest-client",
            "device.mmio.net",
            net,
            DEVICE_MMIO_CAP_SLOT,
        )?;
    }

    // The tree, read-only: the harness reads its boot mode and profile there; gpud, the display-
    // mode authority, the lane's display-mode request (RFC-0098 C7).
    for (pid, svc, subject) in [
        (o.selftest, ServiceId::SelftestClient, "init:selftest-client"),
        (o.gpud, ServiceId::Gpud, "init:gpud"),
        // TASK-0328 U3: the USB host reads its node and the on-board hub's for socd.
        (o.xhcid, ServiceId::Xhcid, "init:xhcid"),
    ] {
        let (tree, slot) =
            (crate::service_topology::NamedSlot::DeviceTree, nexus_abi::INIT_DEVICE_TREE_SLOT);
        let pinned = crate::bootstrap::declared_slots::pin_named(pid, svc, tree, slot, Rights::MAP);
        if pinned.is_some() && iw(init_wire, init_fold, subject) {
            let svc_name = subject.trim_start_matches("init:").as_bytes();
            let line: [&[u8]; 2] = [b"init: device tree grant ok svc=", svc_name];
            crate::bootstrap::diag::emit_marker_atomic(&line, None);
        }
    }

    for (idx, input) in devices.input.iter().copied().enumerate() {
        if let Some(input) = input {
            grant_mmio_with_wait(
                stats,
                pol_route,
                o.hidrawd,
                "hidrawd",
                "device.mmio.input",
                input,
                INPUT_MMIO_CAP_SLOT_BASE + u32::try_from(idx).unwrap_or(0),
            )?;
        }
    }

    // TASK-0328 U1 (RFC-0099): the USB plane — the one host controller to its one owner, then
    // bus mastering for a PCI function (the grant turns it on, for that function only).
    match usb {
        Some(usb) => {
            grant_mmio_with_wait(
                stats,
                pol_route,
                o.xhcid,
                "xhcid",
                "device.mmio.usb",
                usb.window,
                DEVICE_MMIO_CAP_SLOT,
            )?;
            if let Some(function) = usb.pci {
                if let Err(reason) = crate::bootstrap::pci::enable_bus_master(function) {
                    crate::bootstrap::diag::emit_marker_atomic(
                        &[b"init: usb bus master FAIL (", reason.as_bytes(), b")"],
                        None,
                    );
                }
            }
            // TASK-0328 U3: the board's USB 2.0 PHY window, read by xhcid (the stock words
            // compared on the first cycle), under the same class.
            for (phy, slot) in [
                (usb.phy, crate::service_topology::slots::xhcid::PHY),
                (usb.ss_phy, crate::service_topology::slots::xhcid::SS_PHY),
            ] {
                if let Some(phy) = phy {
                    grant_mmio_with_wait(
                        stats,
                        pol_route,
                        o.xhcid,
                        "xhcid",
                        "device.mmio.usb",
                        phy,
                        slot,
                    )?;
                }
            }
        }
        None => debug_write_bytes(b"init: usb plane none (no host controller)\n"),
    }
    Ok(())
}
