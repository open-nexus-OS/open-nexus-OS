// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0315 block-plane wiring (split from `wiring.rs` under the
//! structure ratchet): fixed-slot client wiring (0xF0..0xF2, blockproto
//! SSOT) for statefsd/vfsd and the selftest deny-probe route (a SEPARATE
//! post-wiring pass — see the slot-shift finding in the TASK-0315 ledger).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`init: blk plane wired svc=…` + the gated
//!   blk markers).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::helpers::debug_write_bytes;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::ENDPOINT_FACTORY_CAP_SLOT;
use nexus_abi::Rights;

/// Per-service dispatch inside the spawn-time distribution pass: clients
/// (statefsd/vfsd) get the fixed 0xF0..0xF2 wiring; the owner gets its
/// dedicated IRQ notify endpoint at the fixed slot 0xF1.
pub(crate) fn wire_blk_plane_for(chan: &CtrlChannel, eps: &Endpoints) {
    use crate::service_topology::ServiceId;
    let Some(id) = ServiceId::from_name(chan.svc_name.as_bytes()) else { return };
    match id {
        // TASK-0036-B: bootctld projects the boot record to the `bsb`
        // partition (ADR-0058 runtime writer) over the same fixed-slot
        // plane. TASK-0179: updated writes the INACTIVE boot slot through
        // it (the partition gate in virtioblkd scopes each sender).
        ServiceId::Statefsd | ServiceId::Vfsd | ServiceId::Bootctld | ServiceId::Updated => {
            wire_blk_plane_client(chan.pid, chan.svc_name, eps.vblk_req);
        }
        ServiceId::Virtioblkd => {
            if let Ok(irq_ep) =
                nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, chan.pid, 8)
            {
                let r = nexus_abi::cap_transfer_to_slot(chan.pid, irq_ep, Rights::RECV, 0xF1);
                let _ = nexus_abi::cap_close(irq_ep);
                if r.is_ok() {
                    debug_write_bytes(b"init: blk irq ep wired\n");
                }
            }
        }
        _ => {}
    }
}

/// TASK-0315: block-plane client wiring at FIXED slots (0xF0..0xF2 —
/// blockproto SSOT): virtioblkd request SEND + a dedicated reply pair for
/// statefsd and vfsd. Runs inside the spawn-time distribution so the caps
/// exist BEFORE any request can reach the client services (the statefsd
/// pristine window depends on that ordering).
pub(crate) fn wire_blk_plane_client(pid: u32, name: &str, vblk_req: u32) {
    const REQ_SLOT: u32 = 0xF0;
    const REPLY_RECV_SLOT: u32 = 0xF1;
    const REPLY_SEND_SLOT: u32 = 0xF2;
    let Ok(reply_ep) = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8) else {
        debug_write_bytes(b"init: blk plane reply mint FAIL\n");
        return;
    };
    let a = nexus_abi::cap_transfer_to_slot(pid, vblk_req, Rights::SEND, REQ_SLOT);
    let b = nexus_abi::cap_transfer_to_slot(pid, reply_ep, Rights::RECV, REPLY_RECV_SLOT);
    let c = nexus_abi::cap_transfer_to_slot(pid, reply_ep, Rights::SEND, REPLY_SEND_SLOT);
    let _ = nexus_abi::cap_close(reply_ep);
    if a.is_ok() && b.is_ok() && c.is_ok() {
        debug_write_bytes(b"init: blk plane wired svc=");
        debug_write_bytes(name.as_bytes());
        debug_write_bytes(b"\n");
    } else {
        debug_write_bytes(b"init: blk plane wire FAIL svc=");
        debug_write_bytes(name.as_bytes());
        debug_write_bytes(b"\n");
    }
}

/// TASK-0315: selftest → virtioblkd deny-probe route. A SEPARATE pass that
/// runs AFTER `wire_services`: appending inside the selftest arm would
/// shift its historically fixed slot numbers (0x11/0x12 keystored,
/// 0x17/0x18 reply) and break every hardcoded probe.
pub(crate) fn wire_blk_deny_probe(ctrls: &mut [CtrlChannel], eps: &Endpoints) {
    for chan in ctrls.iter_mut() {
        if chan.svc_name != "selftest-client" {
            continue;
        }
        if let (Ok(bs), Ok(br)) = (
            nexus_abi::cap_transfer(chan.pid, eps.vblk_req, Rights::SEND),
            nexus_abi::cap_transfer(chan.pid, eps.vblk_rsp, Rights::RECV),
        ) {
            chan.set_send(crate::service_topology::ServiceId::Virtioblkd, bs);
            chan.set_recv(crate::service_topology::ServiceId::Virtioblkd, br);
        }
    }
}

/// TASK-0179: updated streams staging containers from the vfsd splice
/// plane — clone the pre-minted vfsd request endpoint so the responder can
/// answer the named route (replies ride the VMO header, not a response
/// queue). Lives here with the rest of the storage-plane wiring rather
/// than growing the bespoke arm in `wiring.rs`.
pub(crate) fn wire_updated_vfs_leg(
    pid: u32,
    chan: &mut CtrlChannel,
    eps: &Endpoints,
    reply_recv_slot: Option<u32>,
) {
    use crate::service_topology::ServiceId;
    let Some((vfs_req, _)) = eps.server_pair(ServiceId::Vfsd) else { return };
    let Ok(clone) = nexus_abi::cap_clone(vfs_req) else { return };
    if let Ok(send_slot) = nexus_abi::cap_transfer(pid, clone, Rights::SEND) {
        chan.set_send(ServiceId::Vfsd, send_slot);
        if let Some(reply_recv_slot) = reply_recv_slot {
            chan.set_recv(ServiceId::Vfsd, reply_recv_slot);
        }
    }
}
