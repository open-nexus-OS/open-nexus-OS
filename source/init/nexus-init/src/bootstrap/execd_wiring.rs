// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: execd's wiring arm and its named routes (TASK-0324 P4e-2, split out of
//! `wiring.rs` + `route_provision.rs` under the module-size ratchet). Every capability
//! execd receives lands in the slot `slots::execd` DECLARES, so nothing in this file
//! depends on the order it runs in — the old arm's order WAS the contract ("keep this
//! block FIRST", "ARM END on purpose"), enforced by comments and a post-mortem slot dump.
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P4)
//! API_STABILITY: Internal
//! TEST_COVERAGE: `nexus-service-topology` slot tests + the QEMU ladder
//! (`init: execd …` wire lines, `execd: recv-wake probe …`, app launch/mount)

use crate::bootstrap::declared_slots;
use crate::bootstrap::diag::iw;
use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::helpers::{debug_write_byte, debug_write_bytes, debug_write_hex};
use crate::bootstrap::CtrlChannel;
use crate::os_payload::{InitError, Result, ENDPOINT_FACTORY_CAP_SLOT};
use crate::service_topology::{NamedSlot, ServiceId};
use nexus_abi::Rights;

/// Wires execd: server pair, reply inbox, the routes it calls and the routes it
/// DELEGATES to the app children it spawns.
pub(crate) fn wire_execd(
    pid: u32,
    eps: &Endpoints,
    chan: &mut CtrlChannel,
    init_wire: &mut nexus_event::SpanTally,
    init_fold: bool,
) -> Result<()> {
    let Endpoints {
        exe_req,
        exe_rsp,
        execd_reply_ep,
        log_req,
        window_req,
        window_rsp,
        bnd_req,
        abil_req,
        sess_req,
        timed_req,
        ..
    } = *eps;
    // TASK-0324 P4e-2: every capability below lands in the slot the topology
    // DECLARES (`slots::execd`), so this arm's order carries no meaning at all.
    // It used to carry all of it: "keep this block FIRST", "ARM END on purpose:
    // transfers here must never shift earlier positional slots", and a
    // `probe_dump_cap_slots()` diagnostic in execd whose only job was to name a
    // drift after it had already produced a dead handshake.
    //
    // Server pair: normally already pinned by the pre-grant pass (task #123).
    if chan.recv(ServiceId::Execd).is_none() || chan.send(ServiceId::Execd).is_none() {
        let slots = declared_slots::pin_server_pair(pid, ServiceId::Execd, exe_req, exe_rsp)
            .ok_or(InitError::Map("execd server slots"))?;
        chan.set_send(ServiceId::Execd, slots.send);
        chan.set_recv(ServiceId::Execd, slots.recv);
    }

    // Reply inbox: RECV stays with execd, SEND is moved to servers per call.
    let inbox = declared_slots::pin_reply_inbox(pid, ServiceId::Execd, execd_reply_ep)
        .ok_or(InitError::Map("execd reply inbox"))?;
    let (reply_recv_slot, reply_send_slot) = (inbox.recv, inbox.send);
    chan.reply_recv_slot = Some(reply_recv_slot);
    chan.reply_send_slot = Some(reply_send_slot);
    let _ = nexus_abi::cap_close(execd_reply_ep);
    if iw(init_wire, init_fold, "init:execd") {
        debug_write_bytes(b"init: execd reply slots recv=0x");
        debug_write_hex(reply_recv_slot as usize);
        debug_write_bytes(b" send=0x");
        debug_write_hex(reply_send_slot as usize);
        debug_write_byte(b'\n');
    }

    // Optional: crash reports to logd via CAP_MOVE (replies ride the inbox).
    if let Some(req) = log_req {
        if let Some(send_slot) =
            declared_slots::pin_route_send(pid, ServiceId::Execd, ServiceId::Logd, req)
        {
            chan.set_send(ServiceId::Logd, send_slot);
            chan.set_recv(ServiceId::Logd, reply_recv_slot);
            if iw(init_wire, init_fold, "init:execd") {
                debug_write_bytes(b"init: execd logd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(reply_recv_slot as usize);
                debug_write_byte(b'\n');
            }
        }
    }
    // ADR-0042 / TASK-0080D R1: the windowd client route execd DELEGATES — execd clones these
    // two caps into every app child's declared pair (`slots::app_child::WINDOWD`) and never
    // calls windowd itself. init pins the originals: a transfer duplicates, so a clone here only
    // leaked an init slot (TASK-0324 P4f-6).
    {
        let (app_send_slot, app_recv_slot) = declared_slots::pin_route(
            pid,
            ServiceId::Execd,
            ServiceId::Windowd,
            window_req,
            window_rsp,
        );
        let (Some(app_send_slot), Some(app_recv_slot)) = (app_send_slot, app_recv_slot) else {
            return Err(InitError::Map("execd->windowd slots"));
        };
        if iw(init_wire, init_fold, "init:execd") {
            debug_write_bytes(b"init: execd windowd slots send=0x");
            debug_write_hex(app_send_slot as usize);
            debug_write_bytes(b" recv=0x");
            debug_write_hex(app_recv_slot as usize);
            debug_write_byte(b'\n');
        }
    }
    // TASK-0080D GET_PAYLOAD: execd fetches ui-program payloads from bundlemgrd
    // for the app processes it spawns (fire-and-forget request + VMO cap move;
    // the child polls the VMO header). RECORD the route (TASK-0080C) — execd
    // re-resolves `bundlemgrd` by name per app launch.
    {
        if let Some(bundle_send_slot) =
            declared_slots::pin_route_send(pid, ServiceId::Execd, ServiceId::Bundlemgrd, bnd_req)
        {
            chan.set_send(ServiceId::Bundlemgrd, bundle_send_slot);
            chan.set_recv(ServiceId::Bundlemgrd, reply_recv_slot);
            if iw(init_wire, init_fold, "init:execd") {
                debug_write_bytes(b"init: execd bundle slot send=0x");
                debug_write_hex(bundle_send_slot as usize);
                debug_write_byte(b'\n');
            }
        }
    }
    // Per-app event channels + per-launch reply inboxes are minted
    // DYNAMICALLY: execd asks init's ctrl plane (`@mint-pair`) and
    // init — the EndpointFactory holder — mints a fresh pair on
    // demand. No static pair, no pre-sized pool (the pool/pair era
    // caused cap-table exhaustion + crossed channels).
    // P0.2 recv-wake regression gate: TWO one-way endpoint pairs for execd's
    // post-ready probe child (a single shared queue would let execd's reply-wait
    // steal its own ping). Both endpoints are pinned to their declared slots.
    {
        let ping_ep = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 4)
            .map_err(InitError::Abi)?;
        let ping_send_slot = declared_slots::pin_named(
            pid,
            ServiceId::Execd,
            NamedSlot::ProbePingSend,
            ping_ep,
            Rights::SEND,
        );
        let ping_recv_slot = declared_slots::pin_named(
            pid,
            ServiceId::Execd,
            NamedSlot::ProbePingRecv,
            ping_ep,
            Rights::RECV,
        );
        let _ = nexus_abi::cap_close(ping_ep);
        let reply_ep = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 4)
            .map_err(InitError::Abi)?;
        let reply_send_slot = declared_slots::pin_named(
            pid,
            ServiceId::Execd,
            NamedSlot::ProbeReplySend,
            reply_ep,
            Rights::SEND,
        );
        let reply_recv_slot = declared_slots::pin_named(
            pid,
            ServiceId::Execd,
            NamedSlot::ProbeReplyRecv,
            reply_ep,
            Rights::RECV,
        );
        let _ = nexus_abi::cap_close(reply_ep);
        if let (Some(ping_send), Some(ping_recv), Some(rep_send), Some(rep_recv)) =
            (ping_send_slot, ping_recv_slot, reply_send_slot, reply_recv_slot)
        {
            if iw(init_wire, init_fold, "init:execd") {
                debug_write_bytes(b"init: execd recv-wake slots ping=0x");
                debug_write_hex(ping_send as usize);
                debug_write_bytes(b"/0x");
                debug_write_hex(ping_recv as usize);
                debug_write_bytes(b" reply=0x");
                debug_write_hex(rep_send as usize);
                debug_write_bytes(b"/0x");
                debug_write_hex(rep_recv as usize);
                debug_write_byte(b'\n');
            }
        }
    }
    // TASK-0080C declarative app-child routing: the named routes execd resolves
    // on behalf of spawned app-hosts (one SEND clone per declared manifest cap →
    // the child's fixed SDK slot, `nexus-sdk-routes`). Recorded once here; the
    // responder answers every `route_ctrl(name)` from these persistent slots.
    if let Some(req) = abil_req {
        if let Some(s) =
            declared_slots::pin_route_send(pid, ServiceId::Execd, ServiceId::Abilitymgr, req)
        {
            chan.set_send(ServiceId::Abilitymgr, s);
            chan.set_recv(ServiceId::Abilitymgr, reply_recv_slot);
            if iw(init_wire, init_fold, "init:execd") {
                debug_write_bytes(b"init: execd route->abilitymgr ok\n");
            }
        }
    }
    if let Some(req) = sess_req {
        if let Some(s) =
            declared_slots::pin_route_send(pid, ServiceId::Execd, ServiceId::Sessiond, req)
        {
            chan.set_send(ServiceId::Sessiond, s);
            chan.set_recv(ServiceId::Sessiond, reply_recv_slot);
            if iw(init_wire, init_fold, "init:execd") {
                debug_write_bytes(b"init: execd route->sessiond ok\n");
            }
        }
    }
    provision_execd_named_routes(pid, eps, timed_req, reply_recv_slot, chan, init_wire, init_fold);

    Ok(())
}

/// execd's `imed-osk` named route (RFC-0075 Phase 2): the DEDICATED osk
/// endpoint — possession IS the authorization; execd provisions it only to
/// `nexus.permission.IME` bundles.
pub(crate) fn provision_execd_imed_osk(
    pid: u32,
    imed_osk: u32,
    reply_recv_slot: u32,
    chan: &mut CtrlChannel,
) {
    if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
        pid,
        ServiceId::Execd,
        ServiceId::ImedOsk,
        imed_osk,
    ) {
        chan.set_send(ServiceId::ImedOsk, s);
        chan.set_recv(ServiceId::ImedOsk, reply_recv_slot);
        if crate::bootstrap::diag::raw_or_expanded("execd") {
            debug_write_bytes(b"init: execd route->imed-osk ok\n");
        }
    }
}

/// The execd NAMED routes (TASK-0080C / RFC-0076 / RFC-0073 / TASK-0049):
/// settingsd, timed, imed-osk, vfsd, statefsd. Split out of the wiring execd
/// arm (module-size ratchet). TASK-0324 P4e-2: every leg is pinned to the slot
/// `slots::execd` declares, so this no longer "must stay AFTER the positional
/// probe/windowd/bundle blocks" — there are no positional blocks left. execd
/// still resolves these BY NAME; the numbers travel in the route response.
#[allow(clippy::too_many_arguments)]
pub(crate) fn provision_execd_named_routes(
    pid: u32,
    eps: &Endpoints,
    timed_req: u32,
    reply_recv_slot: u32,
    chan: &mut CtrlChannel,
    init_wire: &mut nexus_event::SpanTally,
    init_fold: bool,
) {
    // svc.settings.* (DSL settings app / Control Center). Every leg below pins the pre-minted
    // endpoint itself — a transfer duplicates, so the clones these legs used to take only leaked
    // init slots (TASK-0324 P4f-6).
    if let Some((settings_req, _)) = eps.server_pair(ServiceId::Settingsd) {
        if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Execd,
            ServiceId::Settingsd,
            settings_req,
        ) {
            chan.set_send(ServiceId::Settingsd, s);
            chan.set_recv(ServiceId::Settingsd, reply_recv_slot);
            if iw(init_wire, init_fold, "init:execd") {
                if crate::bootstrap::diag::raw_or_expanded("execd") {
                    debug_write_bytes(b"init: execd route->settingsd ok\n");
                }
            }
        }
    }
    // svc.time.* / clock tick (RFC-0076): direct transfer of the
    // pre-minted timed request endpoint (non-consuming). Named
    // route; replies ride the child's CAP_MOVE inbox (timed is
    // ReplyCap-aware).
    if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
        pid,
        ServiceId::Execd,
        ServiceId::Timed,
        timed_req,
    ) {
        chan.set_send(ServiceId::Timed, s);
        chan.set_recv(ServiceId::Timed, reply_recv_slot);
        if iw(init_wire, init_fold, "init:execd") {
            if crate::bootstrap::diag::raw_or_expanded("execd") {
                debug_write_bytes(b"init: execd route->timed ok\n");
            }
        }
    }
    provision_execd_imed_osk(pid, eps.imed_osk, reply_recv_slot, chan);
    // svc.files.* (filemanager role, RFC-0073/TASK-0291): the pre-minted vfsd request endpoint.
    // Named route, replies ride the child's CAP_MOVE inbox (vfsd is ReplyCap-aware).
    if let Some((vfs_req, _)) = eps.server_pair(ServiceId::Vfsd) {
        if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Execd,
            ServiceId::Vfsd,
            vfs_req,
        ) {
            chan.set_send(ServiceId::Vfsd, s);
            chan.set_recv(ServiceId::Vfsd, reply_recv_slot);
            if iw(init_wire, init_fold, "init:execd") {
                if crate::bootstrap::diag::raw_or_expanded("execd") {
                    debug_write_bytes(b"init: execd route->vfsd ok\n");
                }
            }
        }
    }
    // svc.updates.* (settings Updates page, TASK-0140): the pre-minted updated request endpoint.
    // Named route, replies ride the child's CAP_MOVE inbox (updated is ReplyCap-aware);
    // mutating ops stay gated in updated on `updates.manage`.
    if let Some((upd_req, _)) = eps.server_pair(ServiceId::Updated) {
        if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Execd,
            ServiceId::Updated,
            upd_req,
        ) {
            chan.set_send(ServiceId::Updated, s);
            chan.set_recv(ServiceId::Updated, reply_recv_slot);
            if iw(init_wire, init_fold, "init:execd") {
                if crate::bootstrap::diag::raw_or_expanded("execd") {
                    debug_write_bytes(b"init: execd route->updated ok\n");
                }
            }
        }
    }
    // svc.clipboard.* (TASK-0067, RFC-0094): the pre-minted clipboardd request endpoint.
    // Named route, replies ride the child's CAP_MOVE inbox; reads are gated INSIDE
    // clipboardd by windowd's focus truth (the route itself is the write capability).
    if let Some((clip_req, _)) = eps.server_pair(ServiceId::Clipboardd) {
        if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Execd,
            ServiceId::Clipboardd,
            clip_req,
        ) {
            chan.set_send(ServiceId::Clipboardd, s);
            chan.set_recv(ServiceId::Clipboardd, reply_recv_slot);
            if iw(init_wire, init_fold, "init:execd") {
                if crate::bootstrap::diag::raw_or_expanded("execd") {
                    debug_write_bytes(b"init: execd route->clipboardd ok\n");
                }
            }
        }
    }
    // svc.screencap.* (TASK-0068, RFC-0095): the pre-minted screencapd request endpoint. The
    // route is the capability; the packer grants SCREENCAP to the shell and settings only.
    if let Some((scr_req, _)) = eps.server_pair(ServiceId::Screencapd) {
        if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Execd,
            ServiceId::Screencapd,
            scr_req,
        ) {
            chan.set_send(ServiceId::Screencapd, s);
            chan.set_recv(ServiceId::Screencapd, reply_recv_slot);
            if iw(init_wire, init_fold, "init:execd") {
                if crate::bootstrap::diag::raw_or_expanded("execd") {
                    debug_write_bytes(b"init: execd route->screencapd ok\n");
                }
            }
        }
    }
    // TASK-0049 reanimation: execd's statefsd route — (a) execd
    // clones this pair into demo.minidump children BEFORE resume
    // (grant_minidump_statefs_route, child slots 7/8) and (b)
    // execd's own crash-dump writer (`write_dump_to_statefs`)
    // resolves "statefsd" through this table. SharedResponse pair
    // like the dsoftbusd statefs proxy: execd's own wire use is
    // nonce-matched v2; the payload's single v1 PUT rides the
    // quiet exec-phase window.
    if let Some((state_req_ep, state_rsp_ep)) = eps.server_pair(ServiceId::Statefsd) {
        let send = crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Execd,
            ServiceId::Statefsd,
            state_req_ep,
        );
        let recv = crate::bootstrap::declared_slots::pin_route_recv(
            pid,
            ServiceId::Execd,
            ServiceId::Statefsd,
            state_rsp_ep,
        );
        if let (Some(s), Some(r)) = (send, recv) {
            chan.set_send(ServiceId::Statefsd, s);
            chan.set_recv(ServiceId::Statefsd, r);
            if iw(init_wire, init_fold, "init:execd") {
                if crate::bootstrap::diag::raw_or_expanded("execd") {
                    debug_write_bytes(b"init: execd route->statefsd ok\n");
                }
            }
        } else {
            debug_write_bytes(b"init: execd route->statefsd FAIL\n");
        }
    }
    // TASK-0051B: execd's policyd route — the crash writer's attach-level
    // gate (`crash.attach.*`) resolves "policyd" + "@reply" dynamically
    // (nexus_ipc::policyd::check_cap_delegated).
    if let Some((pol_req, _)) = eps.server_pair(ServiceId::Policyd) {
        if let Some(s) = crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Execd,
            ServiceId::Policyd,
            pol_req,
        ) {
            chan.set_send(ServiceId::Policyd, s);
            chan.set_recv(ServiceId::Policyd, reply_recv_slot);
            if iw(init_wire, init_fold, "init:execd") {
                if crate::bootstrap::diag::raw_or_expanded("execd") {
                    debug_write_bytes(b"init: execd route->policyd ok\n");
                }
            }
        }
    }
}
