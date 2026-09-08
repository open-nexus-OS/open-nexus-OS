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

use nexus_abi::yield_;
use nexus_ipc::KernelClient;

use crate::markers::emit_line;
use crate::os_lite::ipc::clients::{cached_netstackd_client, cached_reply_client};

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
    let reply = cached_reply_client().map_err(|_| ())?;
    let (reply_send_slot, reply_recv_slot) = reply.slots();
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
    net.send_with_cap_move(&req, reply_send_clone).map_err(|_| ())?;
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 64];
    let deadline = nexus_abi::nsec().map_err(|_| ())?.saturating_add(3_000_000_000);
    let mut spins: u32 = 0;
    loop {
        spins = spins.wrapping_add(1);
        if spins & 0x3f == 0 && nexus_abi::nsec().map_err(|_| ())? >= deadline {
            return Err(());
        }
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) if n as usize >= 5 && buf[3] == (OP_UDP_BIND | 0x80) => return Ok(buf[4]),
            Ok(_) => {}
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
    }
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
