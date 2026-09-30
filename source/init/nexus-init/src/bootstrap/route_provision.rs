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
/// base.toml). Every leg is pinned to its declared slot (TASK-0324 P4a), so call
/// order no longer decides where anything lands — this helper used to carry
/// "MUST be called AFTER windowd's gpud caps or the present handoff dies".
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
    // TASK-0324 P4a: the inbox and the route land in the slots the topology DECLARES.
    let pinned =
        crate::bootstrap::declared_slots::pin_reply_inbox(pid, ServiceId::Windowd, reply_ep);
    let _ = nexus_abi::cap_close(reply_ep);
    if let Some(inbox) = pinned {
        let (reply_recv, reply_send) = (inbox.recv, inbox.send);
        chan.reply_recv_slot = Some(reply_recv);
        chan.reply_send_slot = Some(reply_send);
        if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Windowd,
            ServiceId::Bundlemgrd,
            bnd_req,
        ) {
            chan.set_send(ServiceId::Bundlemgrd, s);
            chan.set_recv(ServiceId::Bundlemgrd, reply_recv);
            // (emitted from a post-bootstrap helper, outside run_bootstrap's init_wire scope — left raw)
            if crate::bootstrap::diag::raw_or_expanded("windowd") {
                debug_write_bytes(b"init: windowd route->bundlemgrd ok\n");
            }
        }
    }
}

/// Provisions windowd's session route (TASK-0065B): a SEND cap to sessiond's
/// PRE-MINTED request endpoint; replies arrive on the CAP_MOVE reply inbox
/// `provision_windowd_registry_route` created, so that helper still runs first
/// (a data dependency, not a slot-order one: both legs are pinned). Best-effort: a
/// failure leaves the session probe unanswered and windowd falls back to the
/// auto shell — never bricks boot.
pub(crate) fn provision_windowd_session_route(pid: u32, sess_req: u32, chan: &mut CtrlChannel) {
    let Some(reply_recv) = chan.reply_recv_slot else {
        return;
    };
    if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
        pid,
        ServiceId::Windowd,
        ServiceId::Sessiond,
        sess_req,
    ) {
        chan.set_send(ServiceId::Sessiond, s);
        chan.set_recv(ServiceId::Sessiond, reply_recv);
        // (emitted from a post-bootstrap helper, outside run_bootstrap's init_wire scope — left raw)
        if crate::bootstrap::diag::raw_or_expanded("windowd") {
            debug_write_bytes(b"init: windowd route->sessiond ok\n");
        }
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
    if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
        pid,
        ServiceId::Windowd,
        ServiceId::Settingsd,
        settings_req,
    ) {
        chan.set_send(ServiceId::Settingsd, s);
        chan.set_recv(ServiceId::Settingsd, reply_recv);
        if crate::bootstrap::diag::raw_or_expanded("windowd") {
            debug_write_bytes(b"init: windowd route->settingsd ok\n");
        }
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
    match crate::bootstrap::declared_slots::pin_route_send(
        pid,
        ServiceId::Windowd,
        ServiceId::Imed,
        imed_req,
    ) {
        Some(s) => {
            chan.set_send(ServiceId::Imed, s);
            chan.set_recv(ServiceId::Imed, reply_recv);
            if crate::bootstrap::diag::raw_or_expanded("windowd") {
                debug_write_bytes(b"init: windowd route->imed ok\n");
            }
        }
        None => debug_write_bytes(b"init: windowd route->imed FAIL (xfer)\n"),
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
        crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Inputd,
            ServiceId::Imed,
            imed_req,
        ),
        crate::bootstrap::declared_slots::pin_route_recv(
            pid,
            ServiceId::Inputd,
            ServiceId::Imed,
            imed_rsp,
        ),
    ) {
        (Some(s), Some(r)) => {
            chan.set_send(ServiceId::Imed, s);
            chan.set_recv(ServiceId::Imed, r);
            if crate::bootstrap::diag::raw_or_expanded("inputd") {
                debug_write_bytes(b"init: inputd route->imed ok\n");
            }
        }
        _ => debug_write_bytes(b"init: inputd route->imed FAIL (xfer)\n"),
    }
}

/// inputd's declared reply inbox (RFC-0098 C7): windowd answers inputd's ONE display-space
/// call there — never on windowd's shared response endpoint, which several services read.
/// inputd is priority-wired, so the generic declared-legs arm that mints inboxes never runs for
/// it; this is that arm's inbox step, pinned where `slots::inputd::REPLY` declares it.
pub(crate) fn provision_inputd_reply_inbox(pid: u32, chan: &mut CtrlChannel) {
    let factory = crate::os_payload::ENDPOINT_FACTORY_CAP_SLOT;
    let Ok(reply_ep) = nexus_abi::ipc_endpoint_create_for(factory, pid, 8) else {
        debug_write_bytes(b"init: inputd reply inbox FAIL (create)\n");
        return;
    };
    let pinned =
        crate::bootstrap::declared_slots::pin_reply_inbox(pid, ServiceId::Inputd, reply_ep);
    let _ = nexus_abi::cap_close(reply_ep);
    match pinned {
        Some(inbox) => {
            chan.reply_recv_slot = Some(inbox.recv);
            chan.reply_send_slot = Some(inbox.send);
            if crate::bootstrap::diag::raw_or_expanded("inputd") {
                debug_write_bytes(b"init: inputd reply inbox ok\n");
            }
        }
        None => debug_write_bytes(b"init: inputd reply inbox FAIL (pin)\n"),
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
    // Direct transfers (no cap_clone — init's table runs at its ceiling by
    // this point; clones NoSpace-fail and silently killed the launch route).
    match (
        crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Windowd,
            ServiceId::Abilitymgr,
            abil_req,
        ),
        crate::bootstrap::declared_slots::pin_route_recv(
            pid,
            ServiceId::Windowd,
            ServiceId::Abilitymgr,
            abil_rsp,
        ),
    ) {
        (Some(s), Some(r)) => {
            chan.set_send(ServiceId::Abilitymgr, s);
            chan.set_recv(ServiceId::Abilitymgr, r);
            if crate::bootstrap::diag::raw_or_expanded("windowd") {
                debug_write_bytes(b"init: windowd route->abilitymgr ok\n");
            }
        }
        _ => debug_write_bytes(b"init: windowd route->abilitymgr FAIL (xfer)\n"),
    }
}

/// RFC-0076: policy-gated grant of the RTC MMIO window (the node the tree lists by
/// compatible, RFC-0098 C3) to timed — the time authority reads its own wall-clock
/// anchor. ONE waited policy exchange (TASK-0324 P8): a denied/failed/unanswered grant,
/// or a tree without an RTC, leaves walltime honestly UNAVAILABLE, never fatal.
pub(crate) fn grant_rtc_mmio_to_timed(
    timed_pid: u32,
    pol_ctl_route_req: u32,
    pol_ctl_route_rsp: u32,
) -> crate::os_payload::Result<()> {
    use crate::os_payload::{grant_mmio_cap, DEVICE_MMIO_CAP_SLOT};
    let Some(rtc) = crate::bootstrap::device_tree::rtc() else {
        debug_write_bytes(b"init: rtc not in the device tree (walltime unavailable)\n");
        return Ok(());
    };
    if grant_mmio_cap(
        timed_pid,
        "timed",
        "device.mmio.rtc",
        rtc,
        pol_ctl_route_req,
        pol_ctl_route_rsp,
        DEVICE_MMIO_CAP_SLOT,
    )?
    .is_none()
    {
        debug_write_bytes(b"init: rtc mmio grant unavailable\n");
    }
    Ok(())
}

/// imed's two legs the generic route loop cannot find a target endpoint for: the OSK endpoint's
/// RECV half (a dedicated endpoint, not a server pair) and the windowd push route (windowd is
/// priority-wired, so it has no entry in the minted-pair table). Both are pinned where
/// `slots::imed` declares them (TASK-0324 P4f-1b); the settingsd and statefsd legs this function
/// used to hand-build — pinned literals, a `cap_clone` per leg that was never closed — are
/// declared `PrivateInbox` routes provisioned by the generic arm.
pub(crate) fn provision_imed_legs(
    pid: u32,
    imed_osk: u32,
    window_req: u32,
    window_rsp: u32,
    chan: &mut CtrlChannel,
) {
    use crate::service_topology::NamedSlot;
    match crate::bootstrap::declared_slots::pin_named(
        pid,
        ServiceId::Imed,
        NamedSlot::OskServerRecv,
        imed_osk,
        Rights::RECV,
    ) {
        Some(_) => debug_write_bytes(b"init: imed osk recv ok\n"),
        None => debug_write_bytes(b"init: imed osk recv FAIL (xfer)\n"),
    }
    match crate::bootstrap::declared_slots::pin_route(
        pid,
        ServiceId::Imed,
        ServiceId::Windowd,
        window_req,
        window_rsp,
    ) {
        (Some(send), Some(recv)) => {
            chan.set_send(ServiceId::Windowd, send);
            chan.set_recv(ServiceId::Windowd, recv);
            if crate::bootstrap::diag::raw_or_expanded("imed") {
                debug_write_bytes(b"init: imed route->windowd ok\n");
            }
        }
        _ => debug_write_bytes(b"init: imed route->windowd FAIL (xfer)\n"),
    }
}
