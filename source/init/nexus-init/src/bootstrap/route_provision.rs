// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd/inputd client-route provisioning helpers (split out of
//! `wiring.rs`, structure-gate): SEND/RECV cap legs for the registry,
//! session, settings, ability and RFC-0075 imed routes. Pure move — the
//! wiring arms call these; behavior and markers unchanged.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: nexus-init host tests + QEMU boot ladder.

use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::helpers::debug_write_bytes;
use crate::bootstrap::CtrlChannel;
use crate::service_topology::ServiceId;
use nexus_abi::Rights;

/// Provisions windowd's RFC-0065 dynamic-Apps-menu route caps: a CAP_MOVE reply
/// inbox + a SEND cap to bundlemgrd's request endpoint, so windowd's
/// `route_blocking("bundlemgrd")` / `route_blocking("@reply")` resolve (declared in
/// `service_topology` as Windowd→Bundlemgrd; granted `bundle.query`+`ipc.core` in
/// base.toml). MUST be called AFTER windowd's gpud caps are transferred so the
/// present handoff's hardcoded fallback slots (5/6 = gpud) are not displaced.
/// Best-effort: a failure leaves the route unwired (the menu falls back to its
/// seed), never bricks boot.
pub(crate) fn provision_windowd_registry_route(
    factory_slot: u32,
    pid: u32,
    bnd_req: u32,
    chan: &mut CtrlChannel,
) {
    let Ok(reply_ep) = nexus_abi::ipc_endpoint_create_for(factory_slot, pid, 8) else {
        return;
    };
    let rr = nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV);
    let rs = nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND);
    let _ = nexus_abi::cap_close(reply_ep);
    if let (Ok(reply_recv), Ok(reply_send)) = (rr, rs) {
        chan.reply_recv_slot = Some(reply_recv);
        chan.reply_send_slot = Some(reply_send);
        if let Ok(s) = nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND) {
            chan.set_send(ServiceId::Bundlemgrd, s);
            chan.set_recv(ServiceId::Bundlemgrd, reply_recv);
            // (emitted from a post-bootstrap helper, outside run_bootstrap's init_wire scope — left raw)
            debug_write_bytes(b"init: windowd route->bundlemgrd ok\n");
        }
    }
}

/// Provisions windowd's session route (TASK-0065B): a SEND cap to sessiond's
/// PRE-MINTED request endpoint; replies arrive on the CAP_MOVE reply inbox
/// `provision_windowd_registry_route` created — call order matters (that
/// helper first, and both strictly AFTER windowd's gpud caps so the present
/// handoff's hardcoded fallback slots 5/6 are not displaced). Best-effort: a
/// failure leaves the session probe unanswered and windowd falls back to the
/// auto shell — never bricks boot.
pub(crate) fn provision_windowd_session_route(pid: u32, sess_req: u32, chan: &mut CtrlChannel) {
    let Some(reply_recv) = chan.reply_recv_slot else {
        return;
    };
    if let Ok(s) = nexus_abi::cap_transfer(pid, sess_req, Rights::SEND) {
        chan.set_send(ServiceId::Sessiond, s);
        chan.set_recv(ServiceId::Sessiond, reply_recv);
        // (emitted from a post-bootstrap helper, outside run_bootstrap's init_wire scope — left raw)
        debug_write_bytes(b"init: windowd route->sessiond ok\n");
    }
}

/// Provisions windowd's settings route (TASK-0072 Phase 10): a SEND cap to
/// settingsd's PRE-MINTED request endpoint (generic RFC-0069 server pair); the
/// GET/SET replies arrive on the SAME CAP_MOVE reply inbox the registry route
/// created (call order: registry route first). Best-effort: a failure leaves the
/// theme at the build-time default — never bricks boot.
pub(crate) fn provision_windowd_settings_route(
    pid: u32,
    settings_req: u32,
    chan: &mut CtrlChannel,
) {
    let Some(reply_recv) = chan.reply_recv_slot else {
        return;
    };
    if let Ok(s) = nexus_abi::cap_transfer(pid, settings_req, Rights::SEND) {
        chan.set_send(ServiceId::Settingsd, s);
        chan.set_recv(ServiceId::Settingsd, reply_recv);
        debug_write_bytes(b"init: windowd route->settingsd ok\n");
    }
}

/// Provisions windowd's focus-relay route (RFC-0075): SEND on imed's
/// pre-minted request endpoint + the shared reply inbox, so text-focus
/// transitions reach the IME authority. Best-effort: without it, focus
/// relays log a route FAIL and typing stays inert (honest failure).
pub(crate) fn provision_windowd_imed_route(pid: u32, imed_req: u32, chan: &mut CtrlChannel) {
    let Some(reply_recv) = chan.reply_recv_slot else {
        debug_write_bytes(b"init: windowd route->imed FAIL (no reply inbox)\n");
        return;
    };
    // Direct transfer (non-consuming) — a cap_clone would allocate in INIT's
    // cap table, which runs at its 128-slot ceiling by this point in wiring.
    match nexus_abi::cap_transfer(pid, imed_req, Rights::SEND) {
        Ok(s) => {
            chan.set_send(ServiceId::Imed, s);
            chan.set_recv(ServiceId::Imed, reply_recv);
            debug_write_bytes(b"init: windowd route->imed ok\n");
        }
        Err(_) => debug_write_bytes(b"init: windowd route->imed FAIL (xfer)\n"),
    }
}

/// Provisions inputd's key-forward route (RFC-0075): SEND on imed's request
/// endpoint + RECV on its response endpoint (fire-and-forget pushes; the
/// recv side answers name-route lookups). Best-effort.
pub(crate) fn provision_inputd_imed_route(pid: u32, eps: &Endpoints, chan: &mut CtrlChannel) {
    let Some((imed_req, imed_rsp)) = eps.server_pair(ServiceId::Imed) else {
        return;
    };
    match (
        nexus_abi::cap_transfer(pid, imed_req, Rights::SEND),
        nexus_abi::cap_transfer(pid, imed_rsp, Rights::RECV),
    ) {
        (Ok(s), Ok(r)) => {
            chan.set_send(ServiceId::Imed, s);
            chan.set_recv(ServiceId::Imed, r);
            debug_write_bytes(b"init: inputd route->imed ok\n");
        }
        _ => debug_write_bytes(b"init: inputd route->imed FAIL (xfer)\n"),
    }
}

/// Provisions windowd's launch route (TASK-0080D): SEND on abilitymgr's
/// pre-minted request endpoint + RECV on its response endpoint, so the Apps
/// menu's `OP_LAUNCH` reaches the lifecycle broker and the status reply
/// returns. Best-effort: without it, launch requests log a route FAIL.
pub(crate) fn provision_windowd_ability_route(
    pid: u32,
    abil_req: u32,
    abil_rsp: u32,
    chan: &mut CtrlChannel,
) {
    let (Ok(req_clone), Ok(rsp_clone)) =
        (nexus_abi::cap_clone(abil_req), nexus_abi::cap_clone(abil_rsp))
    else {
        return;
    };
    if let (Ok(s), Ok(r)) = (
        nexus_abi::cap_transfer(pid, req_clone, Rights::SEND),
        nexus_abi::cap_transfer(pid, rsp_clone, Rights::RECV),
    ) {
        chan.set_send(ServiceId::Abilitymgr, s);
        chan.set_recv(ServiceId::Abilitymgr, r);
        debug_write_bytes(b"init: windowd route->abilitymgr ok\n");
    }
}
