// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board's glue (TASK-0328 U3, RFC-0099 §6, RFC-0106): the tree names the host node
//! (`snps,dwc3` in host mode), the SuperSpeed PHY (`spacemit,k1x-combphy`: its lane select,
//! a glue word) and the on-board hub (`spacemit,usb3-hub`); socd brings them up — the node's
//! resets, clocks and glue word; the hub's pads, lines, start-up delay and VBUS — before xhcid
//! touches the controller's window (a read of a gated block can stall the bus). A tree
//! without the host node (QEMU: a PCI controller) needs none of it.

use nexus_fdt::Fdt;
use nexus_service_topology::slots;
use nexus_wire::soc;

use crate::os_lite::emit;

/// What the tree said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Glue {
    /// No host node in the tree: nothing to bring up (QEMU's PCI controller).
    NotNeeded,
    /// socd brought the host node (and the hub, if the tree names one) up.
    Up,
    /// socd refused or failed a node: the controller is not touched.
    Failed,
}

const HOST: &[&str] = &["snps,dwc3"];
const HUB: &[&str] = &["spacemit,usb3-hub"];
const SS_PHY: &[&str] = &["spacemit,k1x-combphy"];
const USB2_PHY: &[&str] = &["spacemit,usb2-phy"];
/// The requests' nonces (xhcid's reply inbox is its own: fixed nonces name them).
const NONCE_HOST: u32 = 0x0B01;
const NONCE_HUB: u32 = 0x0B02;
const NONCE_SS_PHY: u32 = 0x0B03;
const NONCE_USB2_PHY: u32 = 0x0B04;

/// Bring the host node and the hub up through socd.
pub(super) fn bring_up() -> Glue {
    let Some(bytes) = nexus_abi::device_tree::map_read_only(slots::xhcid::DEVICE_TREE) else {
        emit(format_args!("xhcid: soc glue not needed (no device tree)"));
        return Glue::NotNeeded;
    };
    let Ok(fdt) = Fdt::new(bytes) else {
        emit(format_args!("xhcid: FAIL (step=glue-tree cc=0)"));
        return Glue::Failed;
    };
    let host = fdt.find_compatible(HOST).find(|node| node.prop_str("dr_mode") == Some("host"));
    let Some(host) = host else {
        emit(format_args!("xhcid: soc glue not needed (no host node in the tree)"));
        return Glue::NotNeeded;
    };
    let hub = fdt.find_compatible(HUB).next();
    let ss_phy = fdt.find_compatible(SS_PHY).next();
    let usb2_phy = fdt.find_compatible(USB2_PHY).next();
    // The nodes socd did not bring up, named in the summary line (" usb2-phy", " ss-phy",
    // " hub"; "" for one that is up).
    let mut not_up = ["", "", ""];
    for (i, (what, node, nonce)) in [
        ("host", Some(host), NONCE_HOST),
        ("usb2-phy", usb2_phy, NONCE_USB2_PHY),
        ("ss-phy", ss_phy, NONCE_SS_PHY),
        ("hub", hub, NONCE_HUB),
    ]
    .into_iter()
    .enumerate()
    {
        let Some(node) = node else {
            emit(format_args!("xhcid: soc glue (no {what} node in the tree)"));
            continue;
        };
        let mut buf = [0u8; 96];
        let Some(path) = node.path_into(&mut buf) else {
            emit(format_args!("xhcid: FAIL (step=glue-{what} cc=1)"));
            return Glue::Failed;
        };
        let reply =
            nexus_ipc::socd::bring_up(slots::xhcid::SOCD.send, slots::xhcid::REPLY, path, nonce);
        let status = match reply {
            Ok(reply) if matches!(reply.status, soc::STATUS_OK | soc::STATUS_NOT_NEEDED) => {
                continue;
            }
            Ok(reply) => reply.status,
            Err(_) => 255,
        };
        emit(format_args!("xhcid: FAIL (step=glue-{what} cc={status})"));
        // The host node's glue gates the controller (its window is dead without it); the hub's
        // powers the ports, the PHYs' clock and release them — without them no device shows,
        // but the controller's own bring-up (the PHYs' words, the core, the reset) still
        // measures, so it goes on (board cycle 5).
        if what == "host" {
            return Glue::Failed;
        }
        not_up[i - 1] = match what {
            "usb2-phy" => " usb2-phy",
            "ss-phy" => " ss-phy",
            _ => " hub",
        };
    }
    if not_up.iter().all(|name| name.is_empty()) {
        emit(format_args!("xhcid: soc glue ok (host + hub up through socd)"));
    } else {
        emit(format_args!(
            "xhcid: soc glue (host up through socd; not up:{}{}{})",
            not_up[0], not_up[1], not_up[2]
        ));
    }
    Glue::Up
}
