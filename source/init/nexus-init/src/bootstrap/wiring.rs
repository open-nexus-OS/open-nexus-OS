// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-service capability distribution — the bespoke + declarative
//! wiring phase init-lite runs after MMIO grants. Extracted verbatim from
//! `orchestrator::run_bootstrap` (RFC-0061 follow-up, task #100).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! ADR: docs/adr/0017-service-architecture.md

use crate::bootstrap::diag::iw;
use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::route_provision::*;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::*;
use crate::service_topology::ServiceId;

/// Distribute capabilities to every spawned service (the bespoke per-service
/// `match` + the declarative generic arm). Mutates each `CtrlChannel`'s slot
/// fields in place; the caller builds the route table from them afterward.
/// RFC-0069 phase semantics (task #123 fix): distribute each declared service's
/// PRE-MINTED server endpoint pair IMMEDIATELY after endpoint creation — before
/// the policy-gated MMIO grant phase. The services' deterministic fallback
/// slots (3/4) then exist no matter how long policyd takes to answer grants;
/// previously a slow policyd delayed `wire_services` past the services'
/// route-probe fallback, their first recv hit an EMPTY slot, and the whole
/// early fleet died (init then aborted wiring caps into dead PIDs). Silent and
/// best-effort; `wire_services` keeps the announce prints + fold tally at the
/// historical log position and skips the pair once set.
pub(crate) fn distribute_server_pairs(ctrls: &mut [CtrlChannel], eps: &Endpoints) {
    for chan in ctrls.iter_mut() {
        let name = chan.svc_name;
        // Authority = the minted-pair table itself (covers declared AND
        // still-bespoke services; returns None for drivers/dsoftbusd).
        let Some(id) = crate::service_topology::ServiceId::from_name(name.as_bytes()) else {
            continue;
        };
        if chan.send(id).is_some() && chan.recv(id).is_some() {
            continue;
        }
        let Some((req, rsp)) = eps.server_pair(id) else {
            // No minted pair: spec-declared plain servers (abilitymgr,
            // sessiond) are provisioned fresh HERE — same pre-grant
            // hardening. Silent; `wire_services` prints the slots at the
            // historical log position from the recorded values.
            if crate::service_topology::exposes_server(name.as_bytes()) && !is_bespoke_wired(name) {
                if let Ok(ep) =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, chan.pid, 8)
                {
                    let recv = nexus_abi::cap_transfer(chan.pid, ep, Rights::RECV);
                    let send = nexus_abi::cap_transfer(chan.pid, ep, Rights::SEND);
                    let _ = nexus_abi::cap_close(ep);
                    if let (Ok(recv_slot), Ok(send_slot)) = (recv, send) {
                        chan.set_send(id, send_slot);
                        chan.set_recv(id, recv_slot);
                    }
                }
            }
            continue;
        };
        let recv = nexus_abi::cap_transfer(chan.pid, req, Rights::RECV);
        let send = nexus_abi::cap_transfer(chan.pid, rsp, Rights::SEND);
        if let (Ok(recv_slot), Ok(send_slot)) = (recv, send) {
            chan.set_send(id, send_slot);
            chan.set_recv(id, recv_slot);
        }
    }
}

pub(crate) fn wire_services(
    ctrls: &mut [CtrlChannel],
    eps: &Endpoints,
    init_fold: bool,
    init_wire: &mut nexus_event::SpanTally,
) -> Result<()> {
    let Endpoints {
        vfs_req,
        vfs_rsp,
        pkg_req,
        pkg_rsp,
        pol_req,
        pol_rsp,
        bnd_req,
        bnd_rsp,
        bnd_rsp_updated,
        bnd_exe_req,
        bnd_exe_rsp,
        upd_req,
        upd_rsp,
        sam_req,
        sam_rsp,
        exe_req,
        exe_rsp,
        key_req,
        key_rsp,
        state_req,
        state_rsp,
        rng_req,
        rng_rsp,
        timed_req,
        timed_rsp,
        window_req,
        window_rsp,
        input_req,
        input_rsp,
        gpud_req,
        gpud_rsp,
        net_req,
        net_rsp,
        net_selftest_rsp,
        net_dsoft_rsp,
        dsoft_req,
        dsoft_rsp,
        dsoft_reply_ep,
        execd_reply_ep,
        reply_ep,
        log_req,
        log_rsp,
        metrics_req,
        metrics_rsp,
        sess_req,
        abil_req,
        abil_rsp,
        ..
    } = *eps;

    // Services are suspended; they will be resumed atomically at the end
    // after all MMIO and IPC wiring is complete.
    let _ = nexus_abi::yield_();

    for chan in ctrls.iter_mut() {
        let pid = chan.pid;
        // Per-service wire-up progress: off by default (probe topic; `INIT_LITE_LOG_TOPICS=probe`).
        if probes_enabled() {
            debug_write_bytes(b"init: wire svc=");
            debug_write_str(chan.svc_name);
            debug_write_bytes(b" pid=0x");
            debug_write_hex(pid as usize);
            debug_write_byte(b'\n');
        }
        match chan.svc_name {
            "netstackd" => {
                // Provide netstackd its own request/response endpoints (server side).
                // #region agent log (netstackd cap transfers)
                if iw(init_wire, init_fold, "init:netstackd") {
                    debug_write_bytes(b"init: wire netstackd xfer net_req RECV\n");
                }
                // #endregion agent log
                // Server pair: usually distributed pre-grants (task #123) — the
                // trace lines stay verbatim (fold-tally parity).
                let recv_slot = match chan.recv(ServiceId::Netstackd) {
                    Some(slot) => slot,
                    None => match nexus_abi::cap_transfer(pid, net_req, Rights::RECV) {
                        Ok(slot) => slot,
                        Err(e) => {
                            // #region agent log (netstackd cap transfer error)
                            debug_write_bytes(b"init: wire netstackd xfer net_req err=abi:");
                            debug_write_str(abi_error_label(e.clone()));
                            debug_write_byte(b'\n');
                            // #endregion agent log
                            return Err(InitError::Abi(e));
                        }
                    },
                };

                // #region agent log (netstackd cap transfers)
                if iw(init_wire, init_fold, "init:netstackd") {
                    debug_write_bytes(b"init: wire netstackd xfer net_rsp SEND\n");
                }
                // #endregion agent log
                let send_slot = match chan.send(ServiceId::Netstackd) {
                    Some(slot) => slot,
                    None => match nexus_abi::cap_transfer(pid, net_rsp, Rights::SEND) {
                        Ok(slot) => slot,
                        Err(e) => {
                            // #region agent log (netstackd cap transfer error)
                            debug_write_bytes(b"init: wire netstackd xfer net_rsp err=abi:");
                            debug_write_str(abi_error_label(e.clone()));
                            debug_write_byte(b'\n');
                            // #endregion agent log
                            return Err(InitError::Abi(e));
                        }
                    },
                };
                chan.set_send(ServiceId::Netstackd, send_slot);
                chan.set_recv(ServiceId::Netstackd, recv_slot);
                if iw(init_wire, init_fold, "init:netstackd") {
                    debug_write_bytes(b"init: netstackd svc slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }
            }
            "dsoftbusd" => {
                // Allow dsoftbusd to send requests to netstackd (and optionally receive on a dedicated inbox).
                // Place into fixed slots to match userspace bring-up constants (avoid relying on allocation order).
                let send_slot = nexus_abi::cap_transfer_to_slot(pid, net_req, Rights::SEND, 0x03)
                    .map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer_to_slot(pid, net_dsoft_rsp, Rights::RECV, 0x04)
                        .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Netstackd, send_slot);
                chan.set_recv(ServiceId::Netstackd, recv_slot);
                if iw(init_wire, init_fold, "init:dsoftbusd") {
                    debug_write_bytes(b"init: dsoftbusd netstackd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Reply inbox: provide both RECV (stay with client) and SEND (to be moved to servers).
                let reply_recv_slot =
                    nexus_abi::cap_transfer_to_slot(pid, dsoft_reply_ep, Rights::RECV, 0x05)
                        .map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer_to_slot(pid, dsoft_reply_ep, Rights::SEND, 0x06)
                        .map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(dsoft_reply_ep);
                if iw(init_wire, init_fold, "init:dsoftbusd") {
                    debug_write_bytes(b"init: dsoftbusd reply slots recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(reply_send_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Allow dsoftbusd to call into samgrd/bundlemgrd via CAP_MOVE reply inbox.
                // - send to service request endpoint
                // - receive replies on local reply inbox recv slot
                let send_slot =
                    nexus_abi::cap_transfer(pid, sam_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Samgrd, send_slot);
                chan.set_recv(ServiceId::Samgrd, reply_recv_slot);
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Bundlemgrd, send_slot);
                chan.set_recv(ServiceId::Bundlemgrd, reply_recv_slot);
                // TASK-0016: remote packagefs RO path requires dsoftbusd -> packagefsd routing.
                let send_slot =
                    nexus_abi::cap_transfer(pid, pkg_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pkg_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Packagefsd, send_slot);
                chan.set_recv(ServiceId::Packagefsd, recv_slot);
                // #region agent log
                if iw(init_wire, init_fold, "init:dsoftbusd") {
                    debug_write_bytes(b"init: dsoftbusd packagefsd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                // #endregion

                // TASK-0017 closeout: allow dsoftbusd to proxy remote statefs via statefsd.
                let send_slot = nexus_abi::cap_transfer(pid, state_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, state_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Statefsd, send_slot);
                chan.set_recv(ServiceId::Statefsd, recv_slot);
                // #region agent log
                if iw(init_wire, init_fold, "init:dsoftbusd") {
                    debug_write_bytes(b"init: dsoftbusd statefsd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                // #endregion

                // Provide dsoftbusd its own request/response endpoints (server side).
                let recv_slot = nexus_abi::cap_transfer(pid, dsoft_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, dsoft_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Dsoftbusd, send_slot);
                chan.set_recv(ServiceId::Dsoftbusd, recv_slot);

                // TASK-0006: allow dsoftbusd to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.set_send(ServiceId::Logd, send_slot);
                    chan.set_recv(ServiceId::Logd, reply_recv_slot);
                }
            }
            // "vfsd" and "packagefsd" migrated to the declarative arm below
            // (RFC-0069 batch 2): spec = SERVICE_SPECS (vfsd's packagefsd link is
            // a SharedResponse route; packagefsd's reply inbox is the pre-minted
            // `pkg_reply_ep`). Their bespoke arms are deleted.
            "policyd" => {
                // Already priority-wired before MMIO grants — skip re-wiring.
                if chan.send(ServiceId::Policyd).is_some()
                    && chan.recv(ServiceId::Policyd).is_some()
                {
                    if iw(init_wire, init_fold, "init:policyd") {
                        debug_write_bytes(b"init: policyd already priority-wired, skip\n");
                    }
                    // Still need reply inbox and logd caps.
                    let pid = chan.pid;
                    let reply_ep =
                        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                            .map_err(|e| {
                                debug_write_bytes(b"init: policyd reply_ep create FAIL\n");
                                InitError::Abi(e)
                            })?;
                    let reply_recv_slot = nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV)
                        .map_err(|e| {
                            debug_write_bytes(b"init: policyd reply_ep xfer RECV FAIL\n");
                            InitError::Abi(e)
                        })?;
                    let reply_send_slot = nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND)
                        .map_err(|e| {
                            debug_write_bytes(b"init: policyd reply_ep xfer SEND FAIL\n");
                            InitError::Abi(e)
                        })?;
                    chan.reply_recv_slot = Some(reply_recv_slot);
                    chan.reply_send_slot = Some(reply_send_slot);
                    chan.set_recv(ServiceId::Statefsd, reply_recv_slot);
                    let _ = nexus_abi::cap_close(reply_ep);
                    if let Some(req) = log_req {
                        let send_slot = nexus_abi::cap_transfer(pid, req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        chan.set_send(ServiceId::Logd, send_slot);
                        chan.set_recv(ServiceId::Logd, reply_recv_slot);
                    }
                } else {
                    let recv_slot = nexus_abi::cap_transfer(pid, pol_req, Rights::RECV)
                        .map_err(InitError::Abi)?;
                    let send_slot = nexus_abi::cap_transfer(pid, pol_rsp, Rights::SEND)
                        .map_err(InitError::Abi)?;
                    chan.set_send(ServiceId::Policyd, send_slot);
                    chan.set_recv(ServiceId::Policyd, recv_slot);
                    if iw(init_wire, init_fold, "init:policyd") {
                        debug_write_bytes(b"init: policyd slots recv=0x");
                        debug_write_hex(recv_slot as usize);
                        debug_write_bytes(b" send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_byte(b'\n');
                    }

                    // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                    let reply_ep =
                        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                            .map_err(InitError::Abi)?;
                    let reply_recv_slot = nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV)
                        .map_err(InitError::Abi)?;
                    let reply_send_slot = nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND)
                        .map_err(InitError::Abi)?;
                    chan.reply_recv_slot = Some(reply_recv_slot);
                    chan.reply_send_slot = Some(reply_send_slot);
                    chan.set_recv(ServiceId::Statefsd, reply_recv_slot);
                    let _ = nexus_abi::cap_close(reply_ep);
                    if iw(init_wire, init_fold, "init:policyd") {
                        debug_write_bytes(b"init: policyd reply slots recv=0x");
                        debug_write_hex(reply_recv_slot as usize);
                        debug_write_bytes(b" send=0x");
                        debug_write_hex(reply_send_slot as usize);
                        debug_write_byte(b'\n');
                    }

                    // TASK-0006: allow policyd to send structured logs to logd via CAP_MOVE (reply inbox).
                    if let Some(req) = log_req {
                        let send_slot = nexus_abi::cap_transfer(pid, req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        chan.set_send(ServiceId::Logd, send_slot);
                        chan.set_recv(ServiceId::Logd, reply_recv_slot);
                        if iw(init_wire, init_fold, "init:policyd") {
                            debug_write_bytes(b"init: policyd logd slots send=0x");
                            debug_write_hex(send_slot as usize);
                            debug_write_bytes(b" recv=0x");
                            debug_write_hex(reply_recv_slot as usize);
                            debug_write_byte(b'\n');
                        }
                    }
                }
            }
            "bundlemgrd" => {
                // Server pair: usually distributed pre-grants (task #123).
                let (recv_slot, send_slot) =
                    match (chan.recv(ServiceId::Bundlemgrd), chan.send(ServiceId::Bundlemgrd)) {
                        (Some(r), Some(s)) => (r, s),
                        _ => {
                            let r = nexus_abi::cap_transfer(pid, bnd_req, Rights::RECV)
                                .map_err(InitError::Abi)?;
                            let s = nexus_abi::cap_transfer(pid, bnd_rsp, Rights::SEND)
                                .map_err(InitError::Abi)?;
                            chan.set_send(ServiceId::Bundlemgrd, s);
                            chan.set_recv(ServiceId::Bundlemgrd, r);
                            (r, s)
                        }
                    };
                if iw(init_wire, init_fold, "init:bundlemgrd") {
                    debug_write_bytes(b"init: bundlemgrd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Allow bundlemgrd to route to execd (policyd may still deny).
                let send_slot = nexus_abi::cap_transfer(pid, bnd_exe_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, bnd_exe_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Execd, send_slot);
                chan.set_recv(ServiceId::Execd, recv_slot);
                let _ = nexus_abi::cap_close(bnd_exe_req);
                let _ = nexus_abi::cap_close(bnd_exe_rsp);

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(reply_ep);

                // TASK-0006: allow bundlemgrd to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.set_send(ServiceId::Logd, send_slot);
                    chan.set_recv(ServiceId::Logd, reply_recv_slot);
                }
            }
            "updated" => {
                // Server pair: usually distributed pre-grants (task #123).
                let (recv_slot, send_slot) =
                    match (chan.recv(ServiceId::Updated), chan.send(ServiceId::Updated)) {
                        (Some(r), Some(s)) => (r, s),
                        _ => {
                            let r = nexus_abi::cap_transfer(pid, upd_req, Rights::RECV)
                                .map_err(InitError::Abi)?;
                            let s = nexus_abi::cap_transfer(pid, upd_rsp, Rights::SEND)
                                .map_err(InitError::Abi)?;
                            chan.set_send(ServiceId::Updated, s);
                            chan.set_recv(ServiceId::Updated, r);
                            (r, s)
                        }
                    };
                if iw(init_wire, init_fold, "init:updated") {
                    debug_write_bytes(b"init: updated slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }

                let transfer = |cap: u32, rights: Rights, label: &'static str| -> Option<u32> {
                    match nexus_abi::cap_transfer(pid, cap, rights) {
                        Ok(slot) => Some(slot),
                        Err(err) => {
                            debug_write_bytes(b"init: updated cap transfer fail ");
                            debug_write_str(label);
                            debug_write_bytes(b" err=");
                            debug_write_str(abi_error_label(err.clone()));
                            debug_write_byte(b'\n');
                            None
                        }
                    }
                };

                // Allow updated to call bundlemgrd (slot-aware publication).
                let send_slot = transfer(bnd_req, Rights::SEND, "bundlemgrd send");
                let recv_slot = transfer(bnd_rsp_updated, Rights::RECV, "bundlemgrd recv");
                if let (Some(send_slot), Some(recv_slot)) = (send_slot, recv_slot) {
                    chan.set_send(ServiceId::Bundlemgrd, send_slot);
                    chan.set_recv(ServiceId::Bundlemgrd, recv_slot);
                }
                let _ = nexus_abi::cap_close(bnd_rsp_updated);

                // Allow updated to call keystored for signature verification.
                let send_slot = transfer(key_req, Rights::SEND, "keystored send");
                let recv_slot = transfer(key_rsp, Rights::RECV, "keystored recv");
                if let (Some(send_slot), Some(recv_slot)) = (send_slot, recv_slot) {
                    chan.set_send(ServiceId::Keystored, send_slot);
                    chan.set_recv(ServiceId::Keystored, recv_slot);
                }

                // Allow updated to call statefsd for persistence.
                let send_slot = transfer(state_req, Rights::SEND, "statefsd send");
                if let Some(send_slot) = send_slot {
                    chan.set_send(ServiceId::Statefsd, send_slot);
                    if iw(init_wire, init_fold, "init:updated") {
                        debug_write_bytes(b"init: updated statefsd send slot=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot = transfer(reply_ep, Rights::RECV, "reply recv");
                let reply_send_slot = transfer(reply_ep, Rights::SEND, "reply send");
                if let (Some(reply_recv_slot), Some(reply_send_slot)) =
                    (reply_recv_slot, reply_send_slot)
                {
                    chan.reply_recv_slot = Some(reply_recv_slot);
                    chan.reply_send_slot = Some(reply_send_slot);
                    chan.set_recv(ServiceId::Statefsd, reply_recv_slot);
                    if iw(init_wire, init_fold, "init:updated") {
                        debug_write_bytes(b"init: updated reply recv slot=0x");
                        debug_write_hex(reply_recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                    if iw(init_wire, init_fold, "init:updated") {
                        debug_write_bytes(b"init: updated reply send slot=0x");
                        debug_write_hex(reply_send_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }
                let _ = nexus_abi::cap_close(reply_ep);

                // TASK-0006: allow updated to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    if let Some(send_slot) = transfer(req, Rights::SEND, "logd send") {
                        chan.set_send(ServiceId::Logd, send_slot);
                        if let Some(reply_recv_slot) = reply_recv_slot {
                            chan.set_recv(ServiceId::Logd, reply_recv_slot);
                        }
                    }
                }
            }
            // "samgrd" migrated to the declarative arm below (RFC-0069 batch 3):
            // announce=true keeps its iw-gated slots line + init_caps tally.
            "execd" => {
                // Server pair: usually distributed pre-grants (task #123).
                if chan.recv(ServiceId::Execd).is_none() || chan.send(ServiceId::Execd).is_none() {
                    let recv_slot = nexus_abi::cap_transfer(pid, exe_req, Rights::RECV)
                        .map_err(InitError::Abi)?;
                    let send_slot = nexus_abi::cap_transfer(pid, exe_rsp, Rights::SEND)
                        .map_err(InitError::Abi)?;
                    chan.set_send(ServiceId::Execd, send_slot);
                    chan.set_recv(ServiceId::Execd, recv_slot);
                }

                // Reply inbox: provide both RECV (stay with execd) and SEND (to be moved to servers).
                let reply_recv_slot = nexus_abi::cap_transfer(pid, execd_reply_ep, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let reply_send_slot = nexus_abi::cap_transfer(pid, execd_reply_ep, Rights::SEND)
                    .map_err(InitError::Abi)?;
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

                // Optional: allow execd to send crash reports to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
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
                // ADR-0042 / TASK-0080D R1: execd forwards a windowd client
                // route to app processes it spawns (clones of these two caps
                // land in the child's fixed slots 5/6). Slot-order contract:
                // execd expects SEND at 8, RECV at 9 (APP_WINDOWD_*_SLOT) —
                // the log line below is the boot-time proof.
                {
                    let window_req_clone =
                        nexus_abi::cap_clone(window_req).map_err(InitError::Abi)?;
                    let window_rsp_clone =
                        nexus_abi::cap_clone(window_rsp).map_err(InitError::Abi)?;
                    let app_send_slot =
                        nexus_abi::cap_transfer(pid, window_req_clone, Rights::SEND)
                            .map_err(InitError::Abi)?;
                    let app_recv_slot =
                        nexus_abi::cap_transfer(pid, window_rsp_clone, Rights::RECV)
                            .map_err(InitError::Abi)?;
                    if iw(init_wire, init_fold, "init:execd") {
                        debug_write_bytes(b"init: execd windowd slots send=0x");
                        debug_write_hex(app_send_slot as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(app_recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }
                // TASK-0080D GET_PAYLOAD: execd fetches ui-program payloads
                // from bundlemgrd for the app processes it spawns (fire-and-
                // forget request + VMO cap move; the child polls the VMO
                // header). Slot-order contract: execd expects SEND at 10
                // (BUNDLE_SEND_SLOT) — the log line is the boot-time proof.
                // CLONE (not move): `bnd_req` stays available for later arms.
                {
                    let bnd_req_clone = nexus_abi::cap_clone(bnd_req).map_err(InitError::Abi)?;
                    let bundle_send_slot =
                        nexus_abi::cap_transfer(pid, bnd_req_clone, Rights::SEND)
                            .map_err(InitError::Abi)?;
                    // RECORD the route (TASK-0080C): execd re-resolves
                    // `bundlemgrd` by name per app launch (child SDK slot
                    // grants) — the route table must answer with this slot.
                    chan.set_send(ServiceId::Bundlemgrd, bundle_send_slot);
                    chan.set_recv(ServiceId::Bundlemgrd, reply_recv_slot);
                    if iw(init_wire, init_fold, "init:execd") {
                        debug_write_bytes(b"init: execd bundle slot send=0x");
                        debug_write_hex(bundle_send_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }
                // Per-app event channels + per-launch reply inboxes are minted
                // DYNAMICALLY: execd asks init's ctrl plane (`@mint-pair`) and
                // init — the EndpointFactory holder — mints a fresh pair on
                // demand. No static pair, no pre-sized pool (the pool/pair era
                // caused cap-table exhaustion + crossed channels).
                // P0.2 recv-wake regression gate: TWO one-way endpoint pairs
                // for execd's post-ready probe child (a single shared queue
                // would let execd's reply-wait steal its own ping). Slot-order
                // contract: execd expects ping SEND at 11, ping RECV at 12,
                // reply SEND at 13, reply RECV at 14 (PROBE_*_SLOT) — the log
                // line below is the boot-time proof. Keep this block FIRST in
                // transfer order after the bundle slot: the probe slots are
                // POSITIONAL (the named-route slots after it are not — their
                // numbers travel in the route response).
                {
                    let ping_ep =
                        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 4)
                            .map_err(InitError::Abi)?;
                    let ping_send_slot = nexus_abi::cap_transfer(pid, ping_ep, Rights::SEND)
                        .map_err(InitError::Abi)?;
                    let ping_recv_slot = nexus_abi::cap_transfer(pid, ping_ep, Rights::RECV)
                        .map_err(InitError::Abi)?;
                    let _ = nexus_abi::cap_close(ping_ep);
                    let reply_ep =
                        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 4)
                            .map_err(InitError::Abi)?;
                    let reply_send_slot = nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND)
                        .map_err(InitError::Abi)?;
                    let reply_recv_slot = nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV)
                        .map_err(InitError::Abi)?;
                    let _ = nexus_abi::cap_close(reply_ep);
                    if iw(init_wire, init_fold, "init:execd") {
                        debug_write_bytes(b"init: execd recv-wake slots ping=0x");
                        debug_write_hex(ping_send_slot as usize);
                        debug_write_bytes(b"/0x");
                        debug_write_hex(ping_recv_slot as usize);
                        debug_write_bytes(b" reply=0x");
                        debug_write_hex(reply_send_slot as usize);
                        debug_write_bytes(b"/0x");
                        debug_write_hex(reply_recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }
                // TASK-0080C declarative app-child routing: the named routes
                // execd resolves on behalf of spawned app-hosts (one SEND
                // clone per declared manifest cap → the child's fixed SDK
                // slot, `nexus-sdk-routes`). Recorded once here; the responder
                // answers every `route_ctrl(name)` from these persistent
                // slots. AFTER the positional probe block on purpose — these
                // slot numbers travel in the route response, so their position
                // is free.
                if let Some(req) = abil_req {
                    let abil_req_clone = nexus_abi::cap_clone(req).map_err(InitError::Abi)?;
                    if let Ok(s) = nexus_abi::cap_transfer(pid, abil_req_clone, Rights::SEND) {
                        chan.set_send(ServiceId::Abilitymgr, s);
                        chan.set_recv(ServiceId::Abilitymgr, reply_recv_slot);
                        if iw(init_wire, init_fold, "init:execd") {
                            debug_write_bytes(b"init: execd route->abilitymgr ok\n");
                        }
                    }
                }
                if let Some(req) = sess_req {
                    if let Ok(s) = nexus_abi::cap_transfer(pid, req, Rights::SEND) {
                        chan.set_send(ServiceId::Sessiond, s);
                        chan.set_recv(ServiceId::Sessiond, reply_recv_slot);
                        if iw(init_wire, init_fold, "init:execd") {
                            debug_write_bytes(b"init: execd route->sessiond ok\n");
                        }
                    }
                }
                // svc.settings.* (DSL settings app / Control Center): CLONE —
                // the pre-minted settingsd request endpoint also serves the
                // windowd arm. Named route (non-positional, behind the probe
                // block like the others).
                if let Some((settings_req, _)) = eps.server_pair(ServiceId::Settingsd) {
                    if let Ok(clone) = nexus_abi::cap_clone(settings_req) {
                        if let Ok(s) = nexus_abi::cap_transfer(pid, clone, Rights::SEND) {
                            chan.set_send(ServiceId::Settingsd, s);
                            chan.set_recv(ServiceId::Settingsd, reply_recv_slot);
                            if iw(init_wire, init_fold, "init:execd") {
                                debug_write_bytes(b"init: execd route->settingsd ok\n");
                            }
                        }
                    }
                }
                // svc.time.* / clock tick (RFC-0076): direct transfer of the
                // pre-minted timed request endpoint (non-consuming). Named
                // route; replies ride the child's CAP_MOVE inbox (timed is
                // ReplyCap-aware).
                if let Ok(s) = nexus_abi::cap_transfer(pid, timed_req, Rights::SEND) {
                    chan.set_send(ServiceId::Timed, s);
                    chan.set_recv(ServiceId::Timed, reply_recv_slot);
                    if iw(init_wire, init_fold, "init:execd") {
                        debug_write_bytes(b"init: execd route->timed ok\n");
                    }
                }
                provision_execd_imed_osk(pid, eps.imed_osk_execd, reply_recv_slot, chan);
                // svc.files.* (filemanager role, RFC-0073/TASK-0291): CLONE of
                // the pre-minted vfsd request endpoint — the generic vfsd arm
                // transfers the original to vfsd itself. Named route, replies
                // ride the child's CAP_MOVE inbox (vfsd is ReplyCap-aware).
                if let Some((vfs_req, _)) = eps.server_pair(ServiceId::Vfsd) {
                    if let Ok(clone) = nexus_abi::cap_clone(vfs_req) {
                        if let Ok(s) = nexus_abi::cap_transfer(pid, clone, Rights::SEND) {
                            chan.set_send(ServiceId::Vfsd, s);
                            chan.set_recv(ServiceId::Vfsd, reply_recv_slot);
                            if iw(init_wire, init_fold, "init:execd") {
                                debug_write_bytes(b"init: execd route->vfsd ok\n");
                            }
                        }
                    }
                }
            }
            "keystored" => {
                // #region agent log (keystored arm entry)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: ks arm\n");
                }
                // #endregion agent log
                // #region agent log (keystored wire-up tracing)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer key_req RECV cap=0x");
                    debug_write_hex(key_req as usize);
                    debug_write_byte(b'\n');
                }
                // #endregion agent log
                // Server pair: usually distributed pre-grants (task #123) — the
                // trace lines above/below stay verbatim (fold-tally parity).
                let recv_slot = match chan.recv(ServiceId::Keystored) {
                    Some(slot) => slot,
                    None => match nexus_abi::cap_transfer(pid, key_req, Rights::RECV) {
                        Ok(slot) => slot,
                        Err(e) => {
                            // #region agent log (keystored wire-up error)
                            debug_write_bytes(b"init: wire keystored xfer key_req err=abi:");
                            debug_write_str(abi_error_label(e.clone()));
                            debug_write_byte(b'\n');
                            // #endregion agent log
                            return Err(InitError::Abi(e));
                        }
                    },
                };

                // #region agent log (keystored wire-up tracing)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer key_rsp SEND cap=0x");
                    debug_write_hex(key_rsp as usize);
                    debug_write_byte(b'\n');
                }
                // #endregion agent log
                let send_slot = match chan.send(ServiceId::Keystored) {
                    Some(slot) => slot,
                    None => match nexus_abi::cap_transfer(pid, key_rsp, Rights::SEND) {
                        Ok(slot) => slot,
                        Err(e) => {
                            // #region agent log (keystored wire-up error)
                            debug_write_bytes(b"init: wire keystored xfer key_rsp err=abi:");
                            debug_write_str(abi_error_label(e.clone()));
                            debug_write_byte(b'\n');
                            // #endregion agent log
                            return Err(InitError::Abi(e));
                        }
                    },
                };
                chan.set_send(ServiceId::Keystored, send_slot);
                chan.set_recv(ServiceId::Keystored, recv_slot);

                // Provide a reply inbox for CAP_MOVE reply routing (used by statefsd + log sinks).
                // #region agent log (keystored reply-inbox create)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored create reply_ep\n");
                }
                // #endregion agent log
                let reply_ep =
                    match nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8) {
                        Ok(slot) => slot,
                        Err(e) => {
                            // #region agent log (keystored wire-up error)
                            debug_write_bytes(b"init: wire keystored create reply_ep err=abi:");
                            debug_write_str(abi_error_label(e.clone()));
                            debug_write_byte(b'\n');
                            // #endregion agent log
                            return Err(InitError::Abi(e));
                        }
                    };

                // #region agent log (keystored reply-inbox transfer)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer reply_ep RECV cap=0x");
                    debug_write_hex(reply_ep as usize);
                    debug_write_byte(b'\n');
                }
                // #endregion agent log
                let reply_recv_slot = match nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer reply_ep RECV err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                // #region agent log (keystored reply-inbox transfer)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer reply_ep SEND cap=0x");
                    debug_write_hex(reply_ep as usize);
                    debug_write_byte(b'\n');
                }
                // #endregion agent log
                let reply_send_slot = match nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer reply_ep SEND err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(reply_ep);

                // statefsd SEND cap + use reply inbox for responses
                // #region agent log (keystored statefsd send cap)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer state_req SEND cap=0x");
                    debug_write_hex(state_req as usize);
                    debug_write_byte(b'\n');
                }
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, state_req, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer state_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.set_send(ServiceId::Statefsd, send_slot);
                chan.set_recv(ServiceId::Statefsd, reply_recv_slot);

                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.set_send(ServiceId::Logd, send_slot);
                    chan.set_recv(ServiceId::Logd, reply_recv_slot);
                }

                // Allow keystored to call policyd (reply via CAP_MOVE/@reply).
                // #region agent log (keystored policyd send cap)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer pol_req SEND cap=0x");
                    debug_write_hex(pol_req as usize);
                    debug_write_byte(b'\n');
                }
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, pol_req, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer pol_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.set_send(ServiceId::Policyd, send_slot);
                chan.set_recv(ServiceId::Policyd, reply_recv_slot);

                // Allow keystored to send entropy requests to rngd (replies via CAP_MOVE/@reply).
                // #region agent log (keystored rngd send cap)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer rng_req SEND cap=0x");
                    debug_write_hex(rng_req as usize);
                    debug_write_byte(b'\n');
                }
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, rng_req, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer rng_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.set_send(ServiceId::Rngd, send_slot);
                // Use reply inbox recv slot for routing responses (CAP_MOVE replies land here).
                chan.set_recv(ServiceId::Rngd, reply_recv_slot);
            }
            // "statefsd" migrated to the declarative arm below (RFC-0069 batch 3):
            // announce=true keeps its iw-gated slots line + init_caps tally.
            // "rngd" and "timed" migrated to the declarative arm below
            // (RFC-0069 batch 1): spec = SERVICE_SPECS, server pair =
            // Endpoints::server_pair. Their bespoke arms are deleted.
            "hidrawd" => {
                let send_slot = nexus_abi::cap_transfer(pid, input_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, input_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Inputd, send_slot);
                chan.set_recv(ServiceId::Inputd, recv_slot);
                if iw(init_wire, init_fold, "init:hidrawd") {
                    debug_write_bytes(b"init: hidrawd inputd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
            }
            "gpud" => {
                let recv_slot = try_transfer(pid, gpud_req, Rights::RECV, "gpud", "RECV");
                let send_slot = try_transfer(pid, gpud_rsp, Rights::SEND, "gpud", "SEND");
                if let (Some(recv), Some(send)) = (recv_slot, send_slot) {
                    chan.set_send(ServiceId::Gpud, send);
                    chan.set_recv(ServiceId::Gpud, recv);
                    if iw(init_wire, init_fold, "init:gpud") {
                        debug_write_bytes(b"init: gpud slots recv=0x");
                        debug_write_hex(recv as usize);
                        debug_write_bytes(b" send=0x");
                        debug_write_hex(send as usize);
                        debug_write_byte(b'\n');
                    }
                }
            }
            "windowd" => {
                // Already priority-wired before MMIO grants — skip re-wiring.
                if chan.send(ServiceId::Windowd).is_some()
                    && chan.recv(ServiceId::Windowd).is_some()
                {
                    if iw(init_wire, init_fold, "init:windowd") {
                        debug_write_bytes(b"init: windowd already priority-wired, skip\n");
                    }
                    // Still need gpud caps.
                    let gpud_send_slot =
                        try_transfer(pid, gpud_req, Rights::SEND, "windowd->gpud", "SEND");
                    let gpud_recv_slot =
                        try_transfer(pid, gpud_rsp, Rights::RECV, "windowd->gpud", "RECV");
                    if let (Some(gpud_send), Some(gpud_recv)) = (gpud_send_slot, gpud_recv_slot) {
                        chan.set_send(ServiceId::Gpud, gpud_send);
                        chan.set_recv(ServiceId::Gpud, gpud_recv);
                    }
                    // RFC-0065 dynamic Apps menu: provision the registry reply-inbox
                    // + bundlemgrd route caps HERE — AFTER the gpud caps, so gpud
                    // keeps the hardcoded fallback slots (5/6) the present handoff
                    // relies on. (Doing this in the priority-wire block shifted gpud
                    // to 8/9 → present handoff `kernel-permission-denied`.)
                    provision_windowd_registry_route(ENDPOINT_FACTORY_CAP_SLOT, pid, bnd_req, chan);
                    // Session route AFTER the registry route (TASK-0065B): it
                    // reuses the reply inbox the registry route just created.
                    if let Some(sess_req) = sess_req {
                        provision_windowd_session_route(pid, sess_req, chan);
                    }
                    // Settings route (TASK-0072 Phase 10): settingsd's minted
                    // request endpoint, same shared reply inbox.
                    if let Some((settings_req, _)) = eps.server_pair(ServiceId::Settingsd) {
                        provision_windowd_settings_route(pid, settings_req, chan);
                    }
                    // Launch route (TASK-0080D): windowd → abilitymgr OP_LAUNCH.
                    if let (Some(req), Some(rsp)) = (abil_req, abil_rsp) {
                        provision_windowd_ability_route(pid, req, rsp, chan);
                    }
                    // Focus relay route (RFC-0075): windowd → imed OP_SET_FOCUS.
                    if let Some((imed_req, _)) = eps.server_pair(ServiceId::Imed) {
                        provision_windowd_imed_route(pid, imed_req, chan);
                        provision_windowd_settings_watch(pid, eps, chan);
                    }
                    continue;
                }
                let recv_slot = nexus_abi::cap_transfer(pid, window_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, window_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Windowd, send_slot);
                chan.set_recv(ServiceId::Windowd, recv_slot);
                // gpud may have crashed — graceful transfer
                let gpud_send_slot =
                    try_transfer(pid, gpud_req, Rights::SEND, "windowd->gpud", "SEND");
                let gpud_recv_slot =
                    try_transfer(pid, gpud_rsp, Rights::RECV, "windowd->gpud", "RECV");
                if let (Some(gpud_send), Some(gpud_recv)) = (gpud_send_slot, gpud_recv_slot) {
                    chan.set_send(ServiceId::Gpud, gpud_send);
                    chan.set_recv(ServiceId::Gpud, gpud_recv);
                }
                // Registry reply-inbox + bundlemgrd route AFTER gpud (slot-order
                // contract — see the skip path above).
                provision_windowd_registry_route(ENDPOINT_FACTORY_CAP_SLOT, pid, bnd_req, chan);
                // Session route AFTER the registry route (TASK-0065B).
                if let Some(sess_req) = sess_req {
                    provision_windowd_session_route(pid, sess_req, chan);
                }
                // Settings route (TASK-0072 Phase 10): settingsd's minted request
                // endpoint, same shared reply inbox as session/registry.
                if let Some((settings_req, _)) = eps.server_pair(ServiceId::Settingsd) {
                    provision_windowd_settings_route(pid, settings_req, chan);
                }
                // Launch route (TASK-0080D): windowd → abilitymgr OP_LAUNCH.
                if let (Some(req), Some(rsp)) = (abil_req, abil_rsp) {
                    provision_windowd_ability_route(pid, req, rsp, chan);
                }
                // Focus relay route (RFC-0075): windowd → imed OP_SET_FOCUS.
                if let Some((imed_req, _)) = eps.server_pair(ServiceId::Imed) {
                    provision_windowd_imed_route(pid, imed_req, chan);
                }
                provision_windowd_settings_watch(pid, eps, chan);
                if iw(init_wire, init_fold, "init:windowd") {
                    debug_write_bytes(b"init: windowd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }
                if let (Some(gpud_send), Some(gpud_recv)) = (gpud_send_slot, gpud_recv_slot) {
                    if iw(init_wire, init_fold, "init:windowd") {
                        debug_write_bytes(b"init: windowd gpud slots send=0x");
                        debug_write_hex(gpud_send as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(gpud_recv as usize);
                        debug_write_byte(b'\n');
                    }
                }
            }
            "inputd" => {
                if chan.send(ServiceId::Inputd).is_some() && chan.recv(ServiceId::Inputd).is_some()
                {
                    if iw(init_wire, init_fold, "init:inputd") {
                        debug_write_bytes(b"init: inputd already priority-wired, skip\n");
                    }
                    // Still need windowd route for visible-state push.
                    let window_send_slot =
                        try_transfer(pid, window_req, Rights::SEND, "inputd->windowd", "SEND");
                    let window_recv_slot =
                        try_transfer(pid, window_rsp, Rights::RECV, "inputd->windowd", "RECV");
                    if let (Some(window_send), Some(window_recv)) =
                        (window_send_slot, window_recv_slot)
                    {
                        chan.set_send(ServiceId::Windowd, window_send);
                        chan.set_recv(ServiceId::Windowd, window_recv);
                        if iw(init_wire, init_fold, "init:inputd") {
                            debug_write_bytes(b"init: inputd windowd slots send=0x");
                            debug_write_hex(window_send as usize);
                            debug_write_bytes(b" recv=0x");
                            debug_write_hex(window_recv as usize);
                            debug_write_byte(b'\n');
                        }
                    }
                    // RFC-0075: key-forward leg to imed — AFTER the windowd
                    // legs (their slot numbers are a boot contract).
                    provision_inputd_imed_route(pid, eps, chan);
                    provision_inputd_settings_watch(pid, eps, chan);
                    continue;
                }
                let recv_slot = nexus_abi::cap_transfer(pid, input_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, input_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Inputd, send_slot);
                chan.set_recv(ServiceId::Inputd, recv_slot);
                let window_send_slot = nexus_abi::cap_transfer(pid, window_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let window_recv_slot = nexus_abi::cap_transfer(pid, window_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Windowd, window_send_slot);
                chan.set_recv(ServiceId::Windowd, window_recv_slot);
                // RFC-0075: key-forward leg to imed (after the windowd legs —
                // their slot numbers are a boot contract).
                provision_inputd_imed_route(pid, eps, chan);
                provision_inputd_settings_watch(pid, eps, chan);
                if iw(init_wire, init_fold, "init:inputd") {
                    debug_write_bytes(b"init: inputd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }
                if iw(init_wire, init_fold, "init:inputd") {
                    debug_write_bytes(b"init: inputd windowd slots send=0x");
                    debug_write_hex(window_send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(window_recv_slot as usize);
                    debug_write_byte(b'\n');
                }
            }
            "metricsd" => {
                if let (Some(req), Some(rsp)) = (metrics_req, metrics_rsp) {
                    // Server pair: usually distributed pre-grants (task #123).
                    let (recv_slot, send_slot) =
                        match (chan.recv(ServiceId::Metricsd), chan.send(ServiceId::Metricsd)) {
                            (Some(r), Some(s)) => (r, s),
                            _ => {
                                let r = nexus_abi::cap_transfer(pid, req, Rights::RECV)
                                    .map_err(InitError::Abi)?;
                                let s = nexus_abi::cap_transfer(pid, rsp, Rights::SEND)
                                    .map_err(InitError::Abi)?;
                                chan.set_send(ServiceId::Metricsd, s);
                                chan.set_recv(ServiceId::Metricsd, r);
                                (r, s)
                            }
                        };
                    if iw(init_wire, init_fold, "init:metricsd") {
                        debug_write_bytes(b"init: metricsd slots recv=0x");
                        debug_write_hex(recv_slot as usize);
                        debug_write_bytes(b" send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sink).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer_to_slot(pid, reply_ep, Rights::RECV, 0x05)
                        .map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer_to_slot(pid, reply_ep, Rights::SEND, 0x06)
                        .map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(reply_ep);

                // Allow metricsd to export snapshots/spans via nexus-log -> logd sink.
                if let Some(req) = log_req {
                    let send_slot = nexus_abi::cap_transfer_to_slot(pid, req, Rights::SEND, 0x08)
                        .map_err(InitError::Abi)?;
                    chan.set_send(ServiceId::Logd, send_slot);
                    chan.set_recv(ServiceId::Logd, reply_recv_slot);
                    if iw(init_wire, init_fold, "init:metricsd") {
                        debug_write_bytes(b"init: metricsd logd slots send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(reply_recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }

                // Allow metricsd retention writer to call statefsd via CAP_MOVE/@reply.
                let send_slot = nexus_abi::cap_transfer_to_slot(pid, state_req, Rights::SEND, 0x07)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Statefsd, send_slot);
                chan.set_recv(ServiceId::Statefsd, reply_recv_slot);
            }
            // "logd" migrated to the declarative arm below (RFC-0069 batch 4):
            // announce=true keeps its iw-gated slots line + init_caps tally; the
            // generic path is best-effort where the old arm aborted init on a
            // failed transfer — the right semantics for a log sink.
            "selftest-client" => {
                let send_slot =
                    nexus_abi::cap_transfer(pid, vfs_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, vfs_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Vfsd, send_slot);
                chan.set_recv(ServiceId::Vfsd, recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest vfsd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, pkg_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pkg_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Packagefsd, send_slot);
                chan.set_recv(ServiceId::Packagefsd, recv_slot);
                let send_slot =
                    nexus_abi::cap_transfer(pid, pol_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pol_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Policyd, send_slot);
                chan.set_recv(ServiceId::Policyd, recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest policyd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, bnd_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Bundlemgrd, send_slot);
                chan.set_recv(ServiceId::Bundlemgrd, recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest bundlemgrd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, upd_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, upd_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Updated, send_slot);
                chan.set_recv(ServiceId::Updated, recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest updated slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, sam_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, sam_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Samgrd, send_slot);
                chan.set_recv(ServiceId::Samgrd, recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest samgrd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, exe_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, exe_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Execd, send_slot);
                chan.set_recv(ServiceId::Execd, recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest execd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, key_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, key_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Keystored, send_slot);
                chan.set_recv(ServiceId::Keystored, recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest keystored slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                let send_slot = nexus_abi::cap_transfer(pid, state_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, state_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Statefsd, send_slot);
                chan.set_recv(ServiceId::Statefsd, recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest statefsd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                if let (Some(req), Some(rsp)) = (log_req, log_rsp) {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    let recv_slot =
                        nexus_abi::cap_transfer(pid, rsp, Rights::RECV).map_err(InitError::Abi)?;
                    chan.set_send(ServiceId::Logd, send_slot);
                    chan.set_recv(ServiceId::Logd, recv_slot);
                    if iw(init_wire, init_fold, "init:selftest-client") {
                        debug_write_bytes(b"init: selftest logd slots send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }
                if let (Some(req), Some(rsp)) = (metrics_req, metrics_rsp) {
                    let send_slot = nexus_abi::cap_transfer_to_slot(pid, req, Rights::SEND, 0x21)
                        .map_err(InitError::Abi)?;
                    let recv_slot = nexus_abi::cap_transfer_to_slot(pid, rsp, Rights::RECV, 0x22)
                        .map_err(InitError::Abi)?;
                    chan.set_send(ServiceId::Metricsd, send_slot);
                    chan.set_recv(ServiceId::Metricsd, recv_slot);
                    if iw(init_wire, init_fold, "init:selftest-client") {
                        debug_write_bytes(b"init: selftest metricsd slots send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }

                // Reply inbox: provide both RECV (stay with client) and SEND (to be moved to servers).
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest reply slots send=0x");
                    debug_write_hex(reply_send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                let send_slot = nexus_abi::cap_transfer(pid, input_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Inputd, send_slot);
                chan.set_recv(ServiceId::Inputd, reply_recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest inputd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Allow selftest-client to send requests to netstackd.
                let send_slot =
                    nexus_abi::cap_transfer(pid, net_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, net_selftest_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Netstackd, send_slot);
                chan.set_recv(ServiceId::Netstackd, recv_slot);

                // Allow selftest-client to send requests to dsoftbusd (TASK-0005 remote proxy proof).
                let send_slot = nexus_abi::cap_transfer(pid, dsoft_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, dsoft_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Dsoftbusd, send_slot);
                chan.set_recv(ServiceId::Dsoftbusd, recv_slot);

                // Allow selftest-client to send requests to rngd and receive direct replies.
                let send_slot =
                    nexus_abi::cap_transfer(pid, rng_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, rng_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Rngd, send_slot);
                chan.set_recv(ServiceId::Rngd, recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest rngd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Allow selftest-client to send requests to timed and receive direct replies.
                let send_slot = nexus_abi::cap_transfer(pid, timed_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, timed_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.set_send(ServiceId::Timed, send_slot);
                chan.set_recv(ServiceId::Timed, recv_slot);

                // Compute broker (SMP track Phase D): LAST in this arm so every
                // auto-assigned slot above keeps its historical number (the
                // deterministic slot constants in the selftest and the wiring
                // guard depend on that order). Best-effort — a missing pinched
                // route is reported by the selftest marker, never an init abort.
                if let Some((pinch_req, pinch_rsp)) = eps.server_pair(ServiceId::Pinched) {
                    if let (Ok(send_slot), Ok(recv_slot)) = (
                        nexus_abi::cap_transfer(pid, pinch_req, Rights::SEND),
                        nexus_abi::cap_transfer(pid, pinch_rsp, Rights::RECV),
                    ) {
                        chan.set_send(ServiceId::Pinched, send_slot);
                        chan.set_recv(ServiceId::Pinched, recv_slot);
                    }
                }
                // Settings-watch probe route (RFC-0078): SEND on settingsd's
                // request endpoint + RECV on its response endpoint (direct
                // transfers — cap-table note above). AFTER pinched/imed so
                // every earlier slot keeps its historical number.
                if let Some((sett_req, sett_rsp)) = eps.server_pair(ServiceId::Settingsd) {
                    match (
                        nexus_abi::cap_transfer(pid, sett_req, Rights::SEND),
                        nexus_abi::cap_transfer(pid, sett_rsp, Rights::RECV),
                    ) {
                        (Ok(send_slot), Ok(recv_slot)) => {
                            chan.set_send(ServiceId::Settingsd, send_slot);
                            chan.set_recv(ServiceId::Settingsd, recv_slot);
                            debug_write_bytes(b"init: selftest route->settingsd ok\n");
                        }
                        _ => debug_write_bytes(b"init: selftest route->settingsd FAIL (xfer)\n"),
                    }
                }
                // IME authority negative probe (RFC-0075): the selftest sends
                // a FOREIGN OP_KEY and must see DENIED. AFTER pinched so every
                // slot above keeps its historical number. Best-effort.
                if let Some((imed_req, imed_rsp)) = eps.server_pair(ServiceId::Imed) {
                    // Direct transfers (no cap_clone — init's cap table runs
                    // at its ceiling here; clones allocate init-side → NoSpace).
                    match (
                        nexus_abi::cap_transfer(pid, imed_req, Rights::SEND),
                        nexus_abi::cap_transfer(pid, imed_rsp, Rights::RECV),
                    ) {
                        (Ok(send_slot), Ok(recv_slot)) => {
                            chan.set_send(ServiceId::Imed, send_slot);
                            chan.set_recv(ServiceId::Imed, recv_slot);
                            debug_write_bytes(b"init: selftest route->imed ok\n");
                            provision_selftest_imed_osk(
                                pid,
                                eps.imed_osk_selftest,
                                recv_slot,
                                chan,
                            );
                        }
                        _ => debug_write_bytes(b"init: selftest route->imed FAIL (xfer)\n"),
                    }
                }
            }
            // RFC-0066 Phase 3 (incremental): services whose wiring is just "a
            // server endpoint" are provisioned **data-driven** from the declarative
            // `ServiceSpec` (host-tested) via the generic helper below — not a
            // bespoke arm. abilitymgr is the first such service; the complex
            // services keep their bespoke arms until they are migrated too.
            name if crate::service_topology::exposes_server(name.as_bytes())
                && !is_bespoke_wired(name) =>
            {
                // Server endpoint: transfer the PRE-MINTED pair when bootstrap
                // created one (its client side is already distributed — a fresh
                // endpoint would orphan those clients); otherwise provision a
                // fresh pair. On success the pre-minted path prints/tallies ONLY
                // where the deleted bespoke arm did (`announce` + the iw() fold
                // tally, RFC-0069 byte-identical migration — iw also increments
                // the `init_caps N/N` count, so it must fire exactly as before).
                let own_id = crate::service_topology::ServiceId::from_name(name.as_bytes());
                let spec = crate::service_topology::spec_for(name.as_bytes());
                let announce = spec.is_some_and(|s| s.announce);
                match own_id.and_then(|id| eps.server_pair(id)) {
                    Some((req, rsp)) => {
                        // Usually already distributed pre-grants (`distribute_
                        // server_pairs`) — announce from the recorded slots so
                        // the marker keeps its historical log position; transfer
                        // here only if the early pass could not.
                        let recorded = own_id.and_then(|id| Some((chan.recv(id)?, chan.send(id)?)));
                        let slots = match recorded {
                            Some(s) => Some(s),
                            None => {
                                let recv = nexus_abi::cap_transfer(pid, req, Rights::RECV);
                                let send = nexus_abi::cap_transfer(pid, rsp, Rights::SEND);
                                match (own_id, recv, send) {
                                    (Some(id), Ok(recv_slot), Ok(send_slot)) => {
                                        chan.set_send(id, send_slot);
                                        chan.set_recv(id, recv_slot);
                                        Some((recv_slot, send_slot))
                                    }
                                    _ => None,
                                }
                            }
                        };
                        // Launch spawn hop (TASK-0080D): the lifecycle broker
                        // is the ONLY app spawner — grant it the execd route.
                        // Push leg (RFC-0075): imed → windowd commit/action
                        // pushes resolve "windowd" by name via this recording.
                        if name == "imed" {
                            provision_imed_legs(
                                pid,
                                eps.imed_osk,
                                window_req,
                                window_rsp,
                                eps.server_pair(ServiceId::Settingsd).map(|(req, _)| req),
                                chan,
                            );
                        }
                        if name == "abilitymgr" {
                            // Direct transfers (no clone — cap-table ceiling).
                            match (
                                nexus_abi::cap_transfer(pid, exe_req, Rights::SEND),
                                nexus_abi::cap_transfer(pid, exe_rsp, Rights::RECV),
                            ) {
                                (Ok(send), Ok(recv)) => {
                                    chan.set_send(ServiceId::Execd, send);
                                    chan.set_recv(ServiceId::Execd, recv);
                                    debug_write_bytes(b"init: abilitymgr route->execd ok\n");
                                }
                                _ => debug_write_bytes(
                                    b"init: abilitymgr route->execd FAIL (xfer)\n",
                                ),
                            }
                        }
                        match slots {
                            Some((recv_slot, send_slot)) => {
                                if announce && iw(init_wire, init_fold, name) {
                                    debug_write_bytes(b"init: ");
                                    debug_write_bytes(name.as_bytes());
                                    debug_write_bytes(b" slots recv=0x");
                                    debug_write_hex(recv_slot as usize);
                                    debug_write_bytes(b" send=0x");
                                    debug_write_hex(send_slot as usize);
                                    debug_write_byte(b'\n');
                                }
                            }
                            None => {
                                debug_write_bytes(b"init: ");
                                debug_write_bytes(name.as_bytes());
                                debug_write_bytes(b" server pair xfer skip\n");
                            }
                        }
                    }
                    None => {
                        // Usually provisioned pre-grants (silent, recorded) —
                        // print the slots here so the marker keeps its
                        // historical position (raw, like the provision print;
                        // NOT iw-gated: this path never counted in the fold
                        // tally). Wire-time provision only as fallback.
                        match own_id.and_then(|id| Some((chan.recv(id)?, chan.send(id)?))) {
                            Some((recv_slot, send_slot)) => {
                                debug_write_bytes(b"init: ");
                                debug_write_bytes(name.as_bytes());
                                debug_write_bytes(b" slots recv=0x");
                                debug_write_hex(recv_slot as usize);
                                debug_write_bytes(b" send=0x");
                                debug_write_hex(send_slot as usize);
                                debug_write_byte(b'\n');
                            }
                            None => provision_server_endpoint(
                                ENDPOINT_FACTORY_CAP_SLOT,
                                pid,
                                name.as_bytes(),
                            ),
                        }
                    }
                }

                // RFC-0066/0069: provision this service's outbound routes **from
                // its declarative `ServiceSpec.routes_to`** (not a bespoke arm).
                // Best-effort: a failure leaves the route unwired, never bricks.
                if let Some(spec) = spec {
                    use crate::service_topology::RouteKind;
                    // CAP_MOVE reply inbox: PRE-MINTED when bootstrap made one,
                    // freshly created otherwise. Same lifecycle either way:
                    // transfer RECV+SEND, close the init-side slot.
                    let mut reply_recv_opt: Option<u32> = None;
                    if !spec.routes_to.is_empty() && spec.reply_inbox {
                        let inbox_ep =
                            own_id.and_then(|id| eps.minted_reply_ep(id)).or_else(|| {
                                nexus_abi::ipc_endpoint_create_for(
                                    ENDPOINT_FACTORY_CAP_SLOT,
                                    pid,
                                    8,
                                )
                                .ok()
                            });
                        if let Some(reply_ep) = inbox_ep {
                            let rr = nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV);
                            let rs = nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND);
                            let _ = nexus_abi::cap_close(reply_ep);
                            if let (Ok(reply_recv), Ok(reply_send)) = (rr, rs) {
                                chan.reply_recv_slot = Some(reply_recv);
                                chan.reply_send_slot = Some(reply_send);
                                reply_recv_opt = Some(reply_recv);
                            }
                        }
                    }
                    for route in spec.routes_to {
                        match route.kind {
                            // Replies arrive on the TARGET's pre-minted response
                            // endpoint, shared directly (vfsd → packagefsd).
                            RouteKind::SharedResponse => {
                                if let Some((t_req, t_rsp)) = eps.server_pair(route.to) {
                                    let s = nexus_abi::cap_transfer(pid, t_req, Rights::SEND);
                                    let r = nexus_abi::cap_transfer(pid, t_rsp, Rights::RECV);
                                    if let (Ok(s), Ok(r)) = (s, r) {
                                        chan.set_send(route.to, s);
                                        chan.set_recv(route.to, r);
                                    }
                                }
                            }
                            // Replies arrive on this service's CAP_MOVE inbox.
                            // Bridge ServiceId → the target's request cap (uniform
                            // target lookup is a later refactor; this reuses what
                            // exists). Prints only where the pre-migration bespoke
                            // arm printed (`announce`) — byte-identical boot logs.
                            RouteKind::ReplyInbox => {
                                let Some(reply_recv) = reply_recv_opt else {
                                    continue;
                                };
                                match route.to {
                                    ServiceId::Bundlemgrd => {
                                        if let Ok(s) =
                                            nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND)
                                        {
                                            chan.set_send(ServiceId::Bundlemgrd, s);
                                            chan.set_recv(ServiceId::Bundlemgrd, reply_recv);
                                            if spec.announce {
                                                debug_write_bytes(b"init: ");
                                                debug_write_bytes(name.as_bytes());
                                                debug_write_bytes(b" route->bundlemgrd ok\n");
                                            }
                                        }
                                    }
                                    ServiceId::Logd => {
                                        if let Some(req) = log_req {
                                            if let Ok(s) =
                                                nexus_abi::cap_transfer(pid, req, Rights::SEND)
                                            {
                                                chan.set_send(ServiceId::Logd, s);
                                                chan.set_recv(ServiceId::Logd, reply_recv);
                                            }
                                        }
                                    }
                                    ServiceId::Policyd => {
                                        if let Ok(s) =
                                            nexus_abi::cap_transfer(pid, pol_req, Rights::SEND)
                                        {
                                            chan.set_send(ServiceId::Policyd, s);
                                            chan.set_recv(ServiceId::Policyd, reply_recv);
                                        }
                                    }
                                    // Session authority (TASK-0065B launch gate).
                                    ServiceId::Sessiond => {
                                        if let Some(req) = sess_req {
                                            if let Ok(s) =
                                                nexus_abi::cap_transfer(pid, req, Rights::SEND)
                                            {
                                                chan.set_send(ServiceId::Sessiond, s);
                                                chan.set_recv(ServiceId::Sessiond, reply_recv);
                                                if spec.announce {
                                                    debug_write_bytes(b"init: ");
                                                    debug_write_bytes(name.as_bytes());
                                                    debug_write_bytes(b" route->sessiond ok\n");
                                                }
                                            }
                                        }
                                    }
                                    // Persistence (settingsd → statefsd): the
                                    // declarative migration left this target
                                    // out of the ReplyInbox arm — every
                                    // `settingsd: … persist=fail` was this
                                    // missing case, not statefsd.
                                    ServiceId::Statefsd => {
                                        if let Ok(s) =
                                            nexus_abi::cap_transfer(pid, state_req, Rights::SEND)
                                        {
                                            chan.set_send(ServiceId::Statefsd, s);
                                            chan.set_recv(ServiceId::Statefsd, reply_recv);
                                            if spec.announce {
                                                debug_write_bytes(b"init: ");
                                                debug_write_bytes(name.as_bytes());
                                                debug_write_bytes(b" route->statefsd ok\n");
                                            }
                                        }
                                    }
                                    // execd route wires with the launch path (later).
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Transfer a capability to a child PID with graceful error handling.
/// Returns Some(slot) on success, None on failure (logs the error).
/// On success, emits a `cap:` hop marker for traceability.
fn try_transfer(pid: u32, cap: u32, rights: Rights, svc: &str, label: &str) -> Option<u32> {
    match nexus_abi::cap_transfer(pid, cap, rights) {
        Ok(slot) => {
            debug_write_bytes(b"cap: route init->");
            debug_write_str(svc);
            debug_write_bytes(b" ");
            debug_write_str(label);
            debug_write_bytes(b" src=0x");
            debug_write_hex(cap as usize);
            debug_write_bytes(b" dst=0x");
            debug_write_hex(slot as usize);
            debug_write_byte(b'\n');
            Some(slot)
        }
        Err(e) => {
            debug_write_bytes(b"init: skip ");
            debug_write_str(svc);
            debug_write_bytes(b" ");
            debug_write_str(label);
            debug_write_bytes(b": ");
            debug_write_str(abi_error_label(e));
            debug_write_byte(b'\n');
            None
        }
    }
}

/// `true` if `name` has a bespoke wiring arm in the orchestrator (complex
/// services with routes/reply-inboxes). RFC-0066 Phase 3: services NOT in this set
/// whose `ServiceSpec.exposes_server` is true are provisioned generically from the
/// declarative topology instead of a hand-written arm. As bespoke services are
/// migrated to `ServiceSpec`, they are removed from this set.
fn is_bespoke_wired(name: &str) -> bool {
    matches!(
        name,
        "netstackd"
            | "dsoftbusd"
            | "policyd"
            | "bundlemgrd"
            | "updated"
            | "execd"
            | "keystored"
            | "hidrawd"
            | "gpud"
            | "windowd"
            | "inputd"
            | "metricsd"
            | "selftest-client"
    )
}

/// Provisions a plain server endpoint for a service (recv/send land at the
/// deterministic fallback slots 3/4 the service expects), driven by the
/// declarative [`crate::service_topology::ServiceSpec`]. Best-effort: a failure
/// leaves the service unwired rather than aborting init — it must never brick boot.
fn provision_server_endpoint(factory_slot: u32, pid: u32, name: &[u8]) {
    match nexus_abi::ipc_endpoint_create_for(factory_slot, pid, 8) {
        Ok(ep) => {
            let recv = nexus_abi::cap_transfer(pid, ep, Rights::RECV);
            let send = nexus_abi::cap_transfer(pid, ep, Rights::SEND);
            let _ = nexus_abi::cap_close(ep);
            match (recv, send) {
                (Ok(recv_slot), Ok(send_slot)) => {
                    debug_write_bytes(b"init: ");
                    debug_write_bytes(name);
                    debug_write_bytes(b" slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }
                _ => {
                    debug_write_bytes(b"init: ");
                    debug_write_bytes(name);
                    debug_write_bytes(b" slot xfer skip\n");
                }
            }
        }
        Err(_) => {
            debug_write_bytes(b"init: ");
            debug_write_bytes(name);
            debug_write_bytes(b" endpoint skip\n");
        }
    }
}
