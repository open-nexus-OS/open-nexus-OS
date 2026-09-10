// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The generic wiring arm (RFC-0093 §4, TASK-0324 P4). A capability lands in the
//! slot the topology DECLARES — `cap_transfer_to_slot`, not "whatever the next free slot is
//! after the previous transfer". The order-based variant was a contract nobody could read:
//! provisioning one route a step earlier moved gpud from 5/6 to 8/9 and the display handoff
//! died with `kernel-permission-denied`, which is why the old arm carried comments begging
//! the next reader not to reorder it.
//!
//! Consumers migrate ONE PER PACKAGE (P4a-P4f). `None` means "this consumer is not declared
//! yet" and the caller keeps its legacy order-based transfer until its package lands.
//!
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P4)
//! API_STABILITY: Internal
//! TEST_COVERAGE: `nexus-service-topology` slot tests + every QEMU lane (a mis-pinned slot
//! is a dead route, not a silent fallback)

use crate::os_payload::{debug_write_byte, debug_write_bytes, debug_write_hex};
use crate::service_topology::{NamedSlot, ServiceId, SlotPair};
use nexus_abi::Rights;

/// Transfers `cap` into the slot the topology declares, or reports loudly.
fn pin(pid: u32, cap: u32, rights: Rights, slot: u32, what: &[u8]) -> Option<u32> {
    match nexus_abi::cap_transfer_to_slot(pid, cap, rights, slot) {
        Ok(landed) => Some(landed),
        Err(_) => {
            debug_write_bytes(b"init: FAIL declared slot ");
            debug_write_bytes(what);
            debug_write_bytes(b" slot=0x");
            debug_write_hex(slot as usize);
            debug_write_byte(b'\n');
            None
        }
    }
}

/// The declared slot pair of `svc`'s own server endpoint.
pub(crate) fn server_slots(svc: ServiceId) -> Option<SlotPair> {
    let spec = crate::service_topology::SERVICE_SPECS.iter().find(|s| s.id == svc)?;
    spec.server_slots.is_declared().then_some(spec.server_slots)
}

/// The declared slot pair of `svc`'s shared CAP_MOVE reply inbox.
pub(crate) fn reply_slots(svc: ServiceId) -> Option<SlotPair> {
    let spec = crate::service_topology::SERVICE_SPECS.iter().find(|s| s.id == svc)?;
    spec.reply_slots.is_declared().then_some(spec.reply_slots)
}

/// Pins `svc`'s server pair (recv + send) into its declared slots.
pub(crate) fn pin_server_pair(pid: u32, svc: ServiceId, req: u32, rsp: u32) -> Option<SlotPair> {
    let slots = server_slots(svc)?;
    let recv = pin(pid, req, Rights::RECV, slots.recv, b"server recv")?;
    let send = pin(pid, rsp, Rights::SEND, slots.send, b"server send")?;
    Some(SlotPair::new(send, recv))
}

/// Pins `svc`'s reply inbox (recv + send halves of one endpoint) into its declared slots.
pub(crate) fn pin_reply_inbox(pid: u32, svc: ServiceId, ep: u32) -> Option<SlotPair> {
    let slots = reply_slots(svc)?;
    let recv = pin(pid, ep, Rights::RECV, slots.recv, b"reply recv")?;
    let send = pin(pid, ep, Rights::SEND, slots.send, b"reply send")?;
    Some(SlotPair::new(send, recv))
}

/// Pins the SEND half of the route `from` → `to` into its declared slot.
pub(crate) fn pin_route_send(pid: u32, from: ServiceId, to: ServiceId, req: u32) -> Option<u32> {
    let slots = crate::service_topology::route_slots(from, to)?;
    pin(pid, req, Rights::SEND, slots.send, b"route send")
}

/// Pins the RECV half of a `SharedResponse` route into its declared slot.
pub(crate) fn pin_route_recv(pid: u32, from: ServiceId, to: ServiceId, rsp: u32) -> Option<u32> {
    let slots = crate::service_topology::route_slots(from, to)?;
    pin(pid, rsp, Rights::RECV, slots.recv, b"route recv")
}

/// Pins BOTH halves of a `SharedResponse` route in one call (the common shape).
pub(crate) fn pin_route(
    pid: u32,
    from: ServiceId,
    to: ServiceId,
    req: u32,
    rsp: u32,
) -> (Option<u32>, Option<u32>) {
    (pin_route_send(pid, from, to, req), pin_route_recv(pid, from, to, rsp))
}

/// Pins a named capability (device MMIO, IRQ notify, a watch channel half, the stage fence).
pub(crate) fn pin_named(
    pid: u32,
    svc: ServiceId,
    name: NamedSlot,
    cap: u32,
    rights: Rights,
) -> Option<u32> {
    let slot = crate::service_topology::extra_slot(svc, name)?;
    pin(pid, cap, rights, slot, b"named slot")
}
