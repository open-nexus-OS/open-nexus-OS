// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Inbound-gateway route provisioning (RFC-0092 / TASK-0052 P3):
//! the selftest's intent route to ingressd. The gateway's own legs
//! (policyd, netstackd) are declarative `ServiceSpec` routes; only the
//! caller side needs a bespoke transfer because the selftest arm is
//! slot-order sensitive. Split out of `route_provision.rs` (module-size
//! ratchet).
//! OWNERS: @runtime @security
//! STATUS: Experimental

use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::helpers::debug_write_bytes;
use crate::bootstrap::CtrlChannel;
use crate::service_topology::ServiceId;
use nexus_abi::Rights;

/// Selftest → ingressd intent route (RFC-0092 / TASK-0052 P3): SEND on the
/// gateway's pre-minted request endpoint (replies ride the selftest's
/// CAP_MOVE inbox) + RECV on the nominal response endpoint so the
/// route-table pair keeps its shape. Wired LAST in the selftest arm so
/// every earlier slot keeps its historical number. Best-effort — a missing
/// route is reported by the selftest marker, never an init abort.
pub(crate) fn provision_selftest_ingress_route(pid: u32, eps: &Endpoints, chan: &mut CtrlChannel) {
    let Some((ingress_req, ingress_rsp)) = eps.server_pair(ServiceId::Ingressd) else {
        return;
    };
    match (
        nexus_abi::cap_transfer(pid, ingress_req, Rights::SEND),
        nexus_abi::cap_transfer(pid, ingress_rsp, Rights::RECV),
    ) {
        (Ok(send_slot), Ok(recv_slot)) => {
            chan.set_send(ServiceId::Ingressd, send_slot);
            chan.set_recv(ServiceId::Ingressd, recv_slot);
            if crate::bootstrap::diag::raw_or_expanded("selftest") {
                debug_write_bytes(b"init: selftest route->ingressd ok\\n");
            }
        }
        _ => debug_write_bytes(b"init: selftest route->ingressd FAIL (xfer)\n"),
    }
}
