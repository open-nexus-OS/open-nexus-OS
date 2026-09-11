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

/// Fixed child slots for inputd's settings-watch channel (RFC-0078). The
/// inputd side hardcodes these (`os_lite.rs` — kept in sync by comment):
/// 0x20 = SEND on settingsd's request endpoint (OP_WATCH + future GETs),
/// 0x21 = RECV of the minted watch channel (event inbox),
/// 0x22 = SEND of the minted watch channel (cap-moved to settingsd inside
/// the OP_WATCH request).
pub(crate) const INPUTD_SETTINGS_SEND_SLOT: u32 = 0x20;

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

/// RFC-0076: policy-gated grant of the goldfish-RTC MMIO window (fixed
/// platform device, dtb-verified `rtc@101000`) to timed — the time authority
/// reads its own wall-clock anchor. Best-effort with a bounded wait: a
/// denied/failed grant leaves walltime honestly UNAVAILABLE, never fatal.
pub(crate) fn grant_rtc_mmio_to_timed(
    timed_pid: u32,
    pol_ctl_route_req: u32,
    pol_ctl_route_rsp: u32,
) -> crate::os_payload::Result<()> {
    use crate::os_payload::{grant_mmio_cap, DEVICE_MMIO_CAP_SLOT};
    const RTC_MMIO_BASE: usize = 0x0010_1000;
    const RTC_MMIO_LEN: usize = 0x1000;
    let deadline = nexus_abi::nsec().map(|n| n.saturating_add(1_000_000_000)).unwrap_or(0);
    loop {
        match grant_mmio_cap(
            timed_pid,
            "timed",
            "device.mmio.rtc",
            RTC_MMIO_BASE,
            RTC_MMIO_LEN,
            pol_ctl_route_req,
            pol_ctl_route_rsp,
            DEVICE_MMIO_CAP_SLOT,
        )? {
            Some(_) => return Ok(()),
            None => {
                if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                    debug_write_bytes(b"init: rtc mmio grant timeout\n");
                    return Ok(());
                }
                let _ = nexus_abi::yield_();
            }
        }
    }
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

/// The selftest harness's `imed-osk` probe route (positive + mis-tag
/// negative); the reply rides the probe's own `@mint-pair` channel, so the
/// recorded recv slot (imed's) is never read.
pub(crate) fn provision_selftest_imed_osk(
    pid: u32,
    imed_osk_selftest: u32,
    recv_slot: u32,
    chan: &mut CtrlChannel,
) {
    match nexus_abi::cap_transfer(pid, imed_osk_selftest, Rights::SEND) {
        Ok(osk_send) => {
            chan.set_send(ServiceId::ImedOsk, osk_send);
            chan.set_recv(ServiceId::ImedOsk, recv_slot);
            if crate::bootstrap::diag::raw_or_expanded("selftest") {
                debug_write_bytes(b"init: selftest route->imed-osk ok\n");
            }
        }
        Err(_) => debug_write_bytes(b"init: selftest route->imed-osk FAIL (xfer)\n"),
    }
}

/// TASK-0140: updated → policyd (the `updates.manage` gate). CLONE of the
/// pre-minted policyd request endpoint (the original serves the fixed-slot
/// arms); the named route resolves at updated's first mutating op; replies
/// ride updated's CAP_MOVE inbox.
pub(crate) fn updated_policyd_leg(
    pid: u32,
    pol_req: u32,
    reply_recv_slot: Option<u32>,
    chan: &mut CtrlChannel,
) {
    if let Ok(clone) = nexus_abi::cap_clone(pol_req) {
        if let Ok(send_slot) = nexus_abi::cap_transfer(pid, clone, Rights::SEND) {
            chan.set_send(ServiceId::Policyd, send_slot);
            if let Some(reply_recv_slot) = reply_recv_slot {
                chan.set_recv(ServiceId::Policyd, reply_recv_slot);
            }
        }
    }
}
