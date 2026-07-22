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

/// Fixed child slots for inputd's settings-watch channel (RFC-0078). The
/// inputd side hardcodes these (`os_lite.rs` — kept in sync by comment):
/// 0x20 = SEND on settingsd's request endpoint (OP_WATCH + future GETs),
/// 0x21 = RECV of the minted watch channel (event inbox),
/// 0x22 = SEND of the minted watch channel (cap-moved to settingsd inside
/// the OP_WATCH request).
const INPUTD_SETTINGS_SEND_SLOT: u32 = 0x20;
const INPUTD_WATCH_RECV_SLOT: u32 = 0x21;
const INPUTD_WATCH_SEND_SLOT: u32 = 0x22;

/// Provisions inputd's settings-watch channel (RFC-0078): a fresh minted
/// endpoint (both halves to inputd at FIXED slots — mint→grant→close, zero
/// init cap-table accumulation) + SEND on settingsd's request endpoint.
/// Best-effort: without it, live keymap switching stays inert (honest
/// failure; the boot keymap default applies).
pub(crate) fn provision_inputd_settings_watch(pid: u32, eps: &Endpoints, chan: &mut CtrlChannel) {
    let Some((settings_req, _)) = eps.server_pair(ServiceId::Settingsd) else {
        return;
    };
    let ok =
        nexus_abi::cap_transfer_to_slot(pid, settings_req, Rights::SEND, INPUTD_SETTINGS_SEND_SLOT)
            .is_ok();
    // Pre-minted in the orchestrator (init's cap table is at its ceiling by
    // wiring time — a late mint NoSpace-fails); init's cap closes after wiring.
    let ep = eps.inputd_watch_ep;
    let recv_ok =
        nexus_abi::cap_transfer_to_slot(pid, ep, Rights::RECV, INPUTD_WATCH_RECV_SLOT).is_ok();
    let send_ok =
        nexus_abi::cap_transfer_to_slot(pid, ep, Rights::SEND, INPUTD_WATCH_SEND_SLOT).is_ok();
    if ok && recv_ok && send_ok {
        chan.set_send(ServiceId::Settingsd, INPUTD_SETTINGS_SEND_SLOT);
        debug_write_bytes(b"init: inputd settings-watch ok\n");
    } else {
        debug_write_bytes(b"init: inputd settings-watch FAIL (xfer)\n");
    }
}

/// Fixed windowd slots for its settings-watch channel (RFC-0076/0077 —
/// windowd relays region data to surfaces). Kept in sync with
/// `windowd/src/compositor/runtime/region.rs`.
const WINDOWD_WATCH_RECV_SLOT: u32 = 0x40;
const WINDOWD_WATCH_SEND_SLOT: u32 = 0x41;

/// Provisions windowd's settings-watch channel (pre-minted; both halves to
/// fixed slots; windowd's settingsd SEND route already exists via
/// `provision_windowd_settings_route`). Best-effort.
pub(crate) fn provision_windowd_settings_watch(pid: u32, eps: &Endpoints, chan: &mut CtrlChannel) {
    let _ = chan;
    let ep = eps.windowd_watch_ep;
    let recv_ok =
        nexus_abi::cap_transfer_to_slot(pid, ep, Rights::RECV, WINDOWD_WATCH_RECV_SLOT).is_ok();
    let send_ok =
        nexus_abi::cap_transfer_to_slot(pid, ep, Rights::SEND, WINDOWD_WATCH_SEND_SLOT).is_ok();
    if recv_ok && send_ok {
        debug_write_bytes(b"init: windowd settings-watch ok\n");
    } else {
        debug_write_bytes(b"init: windowd settings-watch FAIL (xfer)\n");
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
        nexus_abi::cap_transfer(pid, abil_req, Rights::SEND),
        nexus_abi::cap_transfer(pid, abil_rsp, Rights::RECV),
    ) {
        (Ok(s), Ok(r)) => {
            chan.set_send(ServiceId::Abilitymgr, s);
            chan.set_recv(ServiceId::Abilitymgr, r);
            debug_write_bytes(b"init: windowd route->abilitymgr ok\n");
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

/// imed's wiring legs: the OSK-endpoint RECV PINNED to its fixed slot 5
/// (`OSK_RECV_SLOT`, RFC-0075 Phase 2 — before the windowd legs so the
/// number is stable) + the windowd push route (direct transfers, no clone
/// — the cap-table note in `wiring.rs`).
pub(crate) fn provision_imed_legs(
    pid: u32,
    imed_osk: u32,
    window_req: u32,
    window_rsp: u32,
    settings_req: Option<u32>,
    chan: &mut CtrlChannel,
) {
    match nexus_abi::cap_transfer_to_slot(pid, imed_osk, Rights::RECV, 5) {
        Ok(_) => debug_write_bytes(b"init: imed osk recv ok\n"),
        Err(_) => debug_write_bytes(b"init: imed osk recv FAIL (xfer)\n"),
    }
    match (
        nexus_abi::cap_transfer(pid, window_req, Rights::SEND),
        nexus_abi::cap_transfer(pid, window_rsp, Rights::RECV),
    ) {
        (Ok(send), Ok(recv)) => {
            chan.set_send(ServiceId::Windowd, send);
            chan.set_recv(ServiceId::Windowd, recv);
            debug_write_bytes(b"init: imed route->windowd ok\n");
        }
        _ => debug_write_bytes(b"init: imed route->windowd FAIL (xfer)\n"),
    }

    // Layout persistence (RFC-0075 Phase 8b, user decision: the OSK globe
    // switch is SYSTEM-WIDE): imed writes `input.keymap` to settingsd — a
    // SEND clone of the settings request endpoint PINNED to slot 8, plus a
    // private reply inbox (RECV slot 9 / SEND slot 10; imed clones + moves
    // the SEND per OP_SET — mint→grant, zero accumulation).
    if let Some(settings_req) = settings_req {
        let granted = nexus_abi::cap_clone(settings_req)
            .ok()
            .and_then(|clone| nexus_abi::cap_transfer_to_slot(pid, clone, Rights::SEND, 8).ok());
        let reply = nexus_abi::ipc_endpoint_create_for(
            crate::os_payload::ENDPOINT_FACTORY_CAP_SLOT,
            pid,
            4,
        )
        .ok()
        .and_then(|ep| {
            let recv = nexus_abi::cap_transfer_to_slot(pid, ep, Rights::RECV, 9).ok();
            let send = nexus_abi::cap_transfer_to_slot(pid, ep, Rights::SEND, 10).ok();
            let _ = nexus_abi::cap_close(ep);
            recv.and(send)
        });
        if granted.is_some() && reply.is_some() {
            debug_write_bytes(b"init: imed route->settingsd ok\n");
        } else {
            debug_write_bytes(b"init: imed route->settingsd FAIL\n");
        }
    }
}

/// execd's `imed-osk` named route (RFC-0075 Phase 2): the DEDICATED osk
/// endpoint — possession IS the authorization; execd provisions it only to
/// `nexus.permission.IME` bundles. Pre-cloned in the orchestrator (a
/// transfer MOVES the cap).
pub(crate) fn provision_execd_imed_osk(
    pid: u32,
    imed_osk_execd: u32,
    reply_recv_slot: u32,
    chan: &mut CtrlChannel,
) {
    if let Ok(s) = nexus_abi::cap_transfer(pid, imed_osk_execd, Rights::SEND) {
        chan.set_send(ServiceId::ImedOsk, s);
        chan.set_recv(ServiceId::ImedOsk, reply_recv_slot);
        debug_write_bytes(b"init: execd route->imed-osk ok\n");
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
            debug_write_bytes(b"init: selftest route->imed-osk ok\n");
        }
        Err(_) => debug_write_bytes(b"init: selftest route->imed-osk FAIL (xfer)\n"),
    }
}
