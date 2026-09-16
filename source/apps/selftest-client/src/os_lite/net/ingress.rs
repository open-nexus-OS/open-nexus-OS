// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0052 P1 (RFC-0092 Layer A) proof through the real seam: a
//! bind to the NIC-facing address (the facade's bind IP, a port outside the
//! loopback-emulation set) by a subject that is not the ingress gateway is
//! refused by policyd with `STATUS_DENY` — `!cap-deny: enforcer=netstackd
//! class=net.bind port=<p> addr=any subject=0x…`. The loopback bind on the
//! same port range is allowed by the same profile (proved by the QUIC probe's
//! bind). The marker is emitted only after the observed status.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: this probe (QEMU `SELFTEST: ingress deny ok`); host
//!   tests/security_v2_host/ (`test_reject_nonloopback_bind_without_intent`)
//! RFC: docs/rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md

use nexus_ipc::KernelClient;
use nexus_service_topology::slots::selftest_client::REPLY;

use crate::markers::emit_line;
use crate::os_lite::ipc::clients::cached_netstackd_client;

const MAGIC0: u8 = b'N';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;
const OP_UDP_BIND: u8 = 6;
/// netstackd wire: the seam refused the tuple (TASK-0043 P2).
const STATUS_DENY: u8 = 6;
/// Outside the facade's loopback-emulation port set (34567/34568/37020/34569).
const NIC_FACING_PORT: u16 = 40_000;

/// One `udp bind` on `ip:port`; returns the facade's status byte.
fn bind_status(net: &KernelClient, ip: [u8; 4], port: u16) -> core::result::Result<u8, ()> {
    let mut req = [0u8; 10];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_UDP_BIND;
    req[4..8].copy_from_slice(&ip);
    req[8..10].copy_from_slice(&port.to_le_bytes());
    // The harness answers six services into ONE inbox, so the answer is the frame that
    // carries OUR op; anything queued ahead of it is dropped (TASK-0054C P2-c). The 3 s
    // deadline this carried is gone with the spin: the facade's answer or its death ends it.
    let mut buf = [0u8; 64];
    nexus_ipc::exchange::call_matching(net.slots().0, REPLY, &req, &mut buf, |rsp| {
        (rsp.len() >= 5 && rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[3] == (OP_UDP_BIND | 0x80))
            .then(|| rsp[4])
    })
    .map_err(|_| ())
}

/// Runs the Layer-A proof (single-VM; `local_ip` = the facade's bind address).
pub(crate) fn ingress_proofs(local_ip: Option<[u8; 4]>) {
    let Ok(net) = cached_netstackd_client() else {
        emit_line(crate::markers::M_SELFTEST_INGRESS_DENY_FAIL);
        return;
    };
    // The NIC-facing address: the facade's own IP (QEMU user-net 10.0.2.15 by
    // default); a bind there on a non-loopback port is `addr=any` at the seam.
    let ip = local_ip.unwrap_or([10, 0, 2, 15]);
    match bind_status(&net, ip, NIC_FACING_PORT) {
        Ok(STATUS_DENY) => emit_line(crate::markers::M_SELFTEST_INGRESS_DENY_OK),
        _ => emit_line(crate::markers::M_SELFTEST_INGRESS_DENY_FAIL),
    }
}
