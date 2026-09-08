// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: policyd's deterministic client slots (reply inbox 0x9/0xA, logd
//! request 0xB) — PINNED transfers, so no earlier auto-assigned cap can
//! displace them (a displaced 0xB silently lost every audit record and the
//! core-log probe until the selftest markers surfaced it, 2026-09-08).
//! Split out of `wiring.rs` (module-size ratchet).
//! OWNERS: @runtime @security
//! STATUS: Experimental

use crate::bootstrap::helpers::debug_write_bytes;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::{InitError, Result, ENDPOINT_FACTORY_CAP_SLOT};
use crate::service_topology::ServiceId;
use nexus_abi::Rights;

/// Transfers policyd's reply inbox and logd request cap to their pinned slots.
pub(crate) fn pin_policyd_client_slots(
    pid: u32,
    log_req: Option<u32>,
    chan: &mut CtrlChannel,
) -> Result<()> {
    // policyd's deterministic client slots (its audit + core-log
    // probe path never routes): reply inbox RECV 0x9 / SEND 0xA,
    // logd request SEND 0xB — PINNED, so a stray earlier transfer
    // can never displace them again (a displaced 0xB lost every
    // audit record until the selftest markers surfaced it).
    const POLICYD_REPLY_RECV_SLOT: u32 = 0x9;
    const POLICYD_REPLY_SEND_SLOT: u32 = 0xA;
    const POLICYD_LOGD_SEND_SLOT: u32 = 0xB;
    let reply_ep =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8).map_err(|e| {
            debug_write_bytes(b"init: policyd reply_ep create FAIL\n");
            InitError::Abi(e)
        })?;
    let reply_recv_slot =
        nexus_abi::cap_transfer_to_slot(pid, reply_ep, Rights::RECV, POLICYD_REPLY_RECV_SLOT)
            .map_err(|e| {
                debug_write_bytes(b"init: policyd reply_ep xfer RECV FAIL\n");
                InitError::Abi(e)
            })?;
    let reply_send_slot =
        nexus_abi::cap_transfer_to_slot(pid, reply_ep, Rights::SEND, POLICYD_REPLY_SEND_SLOT)
            .map_err(|e| {
                debug_write_bytes(b"init: policyd reply_ep xfer SEND FAIL\n");
                InitError::Abi(e)
            })?;
    chan.reply_recv_slot = Some(reply_recv_slot);
    chan.reply_send_slot = Some(reply_send_slot);
    chan.set_recv(ServiceId::Statefsd, reply_recv_slot);
    let _ = nexus_abi::cap_close(reply_ep);
    if let Some(req) = log_req {
        let send_slot =
            nexus_abi::cap_transfer_to_slot(pid, req, Rights::SEND, POLICYD_LOGD_SEND_SLOT)
                .map_err(|e| {
                    debug_write_bytes(b"init: policyd logd xfer FAIL\n");
                    InitError::Abi(e)
                })?;
        chan.set_send(ServiceId::Logd, send_slot);
        chan.set_recv(ServiceId::Logd, reply_recv_slot);
    }
    Ok(())
}
