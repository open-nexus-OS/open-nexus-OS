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

/// Per-service dispatch inside the spawn-time distribution pass: clients get the block
/// plane's fleet slots (`BLK_PLANE_REQ_SLOT` + `BLK_PLANE_REPLY`); the owner gets its dedicated
/// IRQ notify endpoint in its declared named slot (TASK-0324 P4f-1b).
pub(crate) fn wire_blk_plane_for(chan: &CtrlChannel, eps: &Endpoints) {
    wire_blk_plane_for_with(chan, eps.blk_req);
}

/// The same dispatch from the bare request endpoint — the CORE-plane stage
/// (TASK-0321 P4) wires bundlemgrd/blkd before `Endpoints` exists.
pub(crate) fn wire_blk_plane_for_with(chan: &CtrlChannel, blk_req: u32) {
    use crate::service_topology::ServiceId;
    let Some(id) = ServiceId::from_name(chan.svc_name.as_bytes()) else { return };
    match id {
        // TASK-0036-B: bootctld projects the boot record to the `bsb`
        // partition (ADR-0058 runtime writer) over the same fixed-slot
        // plane. TASK-0179: updated writes the INACTIVE boot slot through
        // it (the partition gate in blkd scopes each sender).
        // TASK-0321 (RFC-0089 §12.3/§12.5): bundlemgrd READS the system
        // volume (NXSV + index + bundle windows) over the same fixed-slot
        // plane; the op-aware gate in blkd denies it every write.
        ServiceId::Statefsd
        | ServiceId::Vfsd
        | ServiceId::Bootctld
        | ServiceId::Updated
        | ServiceId::Bundlemgrd => {
            wire_blk_plane_client(chan.pid, chan.svc_name, blk_req);
        }
        ServiceId::Blkd => {
            if let Ok(irq_ep) =
                nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, chan.pid, 8)
            {
                let r = crate::bootstrap::declared_slots::pin_named(
                    chan.pid,
                    ServiceId::Blkd,
                    crate::service_topology::NamedSlot::IrqNotify,
                    irq_ep,
                    Rights::RECV,
                );
                let _ = nexus_abi::cap_close(irq_ep);
                if r.is_some() {
                    debug_write_bytes(b"init: blk irq ep wired\n");
                }
            }
        }
        _ => {}
    }
}

/// TASK-0315: block-plane client wiring at the fleet's block-plane slots (declared once in
/// `nexus-service-topology`, read by `storage::blockproto` too): blkd request SEND + a
/// dedicated reply pair per client. Runs inside the spawn-time distribution so the caps
/// exist BEFORE any request can reach the client services (the statefsd
/// pristine window depends on that ordering).
pub(crate) fn wire_blk_plane_client(pid: u32, name: &str, blk_req: u32) {
    use crate::service_topology::{BLK_PLANE_REPLY, BLK_PLANE_REQ_SLOT};
    let Ok(reply_ep) = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8) else {
        debug_write_bytes(b"init: blk plane reply mint FAIL\n");
        return;
    };
    let a = nexus_abi::cap_transfer_to_slot(pid, blk_req, Rights::SEND, BLK_PLANE_REQ_SLOT);
    let b = nexus_abi::cap_transfer_to_slot(pid, reply_ep, Rights::RECV, BLK_PLANE_REPLY.recv);
    let c = nexus_abi::cap_transfer_to_slot(pid, reply_ep, Rights::SEND, BLK_PLANE_REPLY.send);
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
