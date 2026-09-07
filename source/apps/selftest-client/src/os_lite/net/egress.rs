// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0043 P3 egress proof over the real seam: `connect` requests
//! through netstackd's facade are decided by policyd against this subject's
//! RFC-0091 `net.connect` profile (`10.0.2.0/24`, ports 53/80/443). A target
//! outside the CIDR and one on a refused port answer `STATUS_DENY` from the
//! seam (never dialled); an allowed target is admitted (any non-deny status —
//! the dial outcome is the network's, not the policy's). Under Learn mode the
//! refusal is admitted by policyd's learn collector (`OP_ABI_LEARN_STATS`
//! +1). Markers are emitted only after the observed statuses.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: this probe (QEMU `SELFTEST: egress deny/allow ok`);
//!   host tests/security_v2_host/
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use nexus_abi::yield_;
use nexus_ipc::KernelClient;

use crate::markers::emit_line;
use crate::os_lite::ipc::clients::{cached_netstackd_client, cached_reply_client};
use crate::os_lite::ipc::routing::route_with_retry;
use crate::os_lite::services;

const MAGIC0: u8 = b'N';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;
const OP_CONNECT: u8 = 3;
/// netstackd wire: the seam refused the tuple (TASK-0043 P2).
const STATUS_DENY: u8 = 6;

/// One `connect` request; returns the facade's status byte.
fn connect_status(net: &KernelClient, ip: [u8; 4], port: u16) -> core::result::Result<u8, ()> {
    let mut req = [0u8; 10];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_CONNECT;
    req[4..8].copy_from_slice(&ip);
    req[8..10].copy_from_slice(&port.to_le_bytes());
    let reply = cached_reply_client().map_err(|_| ())?;
    let (reply_send_slot, reply_recv_slot) = reply.slots();
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
    net.send_with_cap_move(&req, reply_send_clone).map_err(|_| ())?;
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 64];
    // Time-based budget (the facade may be mid-dial or mid-ping for a while).
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
            Ok(n) if n as usize >= 5 && buf[3] == (OP_CONNECT | 0x80) => return Ok(buf[4]),
            Ok(_) => {}
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
    }
}

/// Runs the egress proofs (single-VM profile: QEMU user-net gateway 10.0.2.2).
pub(crate) fn egress_proofs() {
    let Ok(net) = cached_netstackd_client() else {
        emit_line(crate::markers::M_SELFTEST_EGRESS_DENY_FAIL);
        emit_line(crate::markers::M_SELFTEST_EGRESS_ALLOW_FAIL);
        emit_line(crate::markers::M_SELFTEST_EGRESS_LEARN_COLLECTED_FAIL);
        return;
    };
    // Outside the allowed CIDR, and inside the CIDR on a refused port.
    let cidr_deny = connect_status(&net, [192, 168, 1, 1], 80);
    let port_deny = connect_status(&net, [10, 0, 2, 2], 8080);
    if cidr_deny == Ok(STATUS_DENY) && port_deny == Ok(STATUS_DENY) {
        emit_line(crate::markers::M_SELFTEST_EGRESS_DENY_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_EGRESS_DENY_FAIL);
    }
    // Allowed tuple: the seam admits; whatever the dial returns, it is not DENY.
    match connect_status(&net, [10, 0, 2, 2], 53) {
        Ok(status) if status != STATUS_DENY => {
            emit_line(crate::markers::M_SELFTEST_EGRESS_ALLOW_OK);
        }
        _ => emit_line(crate::markers::M_SELFTEST_EGRESS_ALLOW_FAIL),
    }
    // Learn mode: the same refusal is admitted by policyd's collector (+1),
    // decision unchanged (still DENY at the seam). Refusals are never cached
    // by netstackd, so policyd sees this one.
    let learned = (|| {
        use nexus_abi::policyd::{ABI_MODE_ENFORCE, ABI_MODE_LEARN, STATUS_ALLOW};
        let policyd = route_with_retry("policyd").ok()?;
        let sid = nexus_abi::service_id_from_name(b"selftest-client");
        let profile = services::policyd::policyd_fetch_abi_profile(&policyd, sid).ok()?;
        let epoch = profile.epoch();
        if services::policyd::policyd_set_abi_mode(&policyd, sid, ABI_MODE_LEARN, epoch)
            != Ok(STATUS_ALLOW)
        {
            return None;
        }
        let before = services::policyd::policyd_abi_learn_stats(&policyd, sid).ok()?;
        let status = connect_status(&net, [192, 168, 1, 2], 443);
        let after = services::policyd::policyd_abi_learn_stats(&policyd, sid).ok()?;
        let back = services::policyd::policyd_set_abi_mode(&policyd, sid, ABI_MODE_ENFORCE, epoch);
        Some(status == Ok(STATUS_DENY) && after.1 == before.1 + 1 && back == Ok(STATUS_ALLOW))
    })();
    if learned == Some(true) {
        emit_line(crate::markers::M_SELFTEST_EGRESS_LEARN_COLLECTED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_EGRESS_LEARN_COLLECTED_FAIL);
    }
}
