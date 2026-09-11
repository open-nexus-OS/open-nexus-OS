// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-service capability distribution — the bespoke + declarative
//! wiring phase init-lite runs after MMIO grants. Extracted verbatim from
//! `orchestrator::run_bootstrap` (RFC-0061 follow-up, task #100).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! ADR: docs/adr/0017-service-architecture.md

use crate::bootstrap::declared_routes;
use crate::bootstrap::declared_slots;
use crate::bootstrap::diag::iw;
use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::execd_wiring;
use crate::bootstrap::route_provision::*;
use crate::bootstrap::settings_watch_route::*;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::*;
use crate::service_topology::ServiceId;

pub(crate) fn wire_services(
    ctrls: &mut [CtrlChannel],
    eps: &Endpoints,
    init_fold: bool,
    init_wire: &mut nexus_event::SpanTally,
) -> Result<()> {
    let Endpoints {
        bnd_req,
        window_req,
        window_rsp,
        input_req,
        input_rsp,
        gpud_req,
        gpud_rsp,
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
            // "vfsd" and "packagefsd" migrated to the declarative arm below
            // (RFC-0069 batch 2): spec = SERVICE_SPECS (vfsd's packagefsd link is
            // a SharedResponse route; packagefsd's reply inbox is the pre-minted
            // `pkg_reply_ep`). Their bespoke arms are deleted.
            // "bootctld" is provisioned by the generic arm from its declaration (TASK-0324 P4f-1b).
            // "netstackd", "dsoftbusd" and "metricsd" too (TASK-0324 P4f-4).
            // "selftest-client" is pinned before wave 1 resumes it (`declared_routes`, P4f-5).
            "execd" => execd_wiring::wire_execd(pid, eps, chan, init_wire, init_fold)?,
            "hidrawd" => {
                // TASK-0324 P4d: pinned to the declared slots (hidrawd is a pure producer).
                let (send_slot, recv_slot) = declared_slots::pin_route(
                    pid,
                    ServiceId::Hidrawd,
                    ServiceId::Inputd,
                    input_req,
                    input_rsp,
                );
                let (Some(send_slot), Some(recv_slot)) = (send_slot, recv_slot) else {
                    return Err(InitError::Map("hidrawd->inputd slots"));
                };
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
                // TASK-0324 P4c: pinned to the declared slots — gpud hardcodes the same
                // pair on its side, and windowd's whole present path depends on it.
                let pinned =
                    declared_slots::pin_server_pair(pid, ServiceId::Gpud, gpud_req, gpud_rsp);
                if let Some(slots) = pinned {
                    let (recv, send) = (slots.recv, slots.send);
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
                    // Still need gpud caps — PINNED to the declared slots (TASK-0324 P4a).
                    let (gpud_send_slot, gpud_recv_slot) = declared_slots::pin_route(
                        pid,
                        ServiceId::Windowd,
                        ServiceId::Gpud,
                        gpud_req,
                        gpud_rsp,
                    );
                    if let (Some(gpud_send), Some(gpud_recv)) = (gpud_send_slot, gpud_recv_slot) {
                        chan.set_send(ServiceId::Gpud, gpud_send);
                        chan.set_recv(ServiceId::Gpud, gpud_recv);
                    }
                    // RFC-0065 dynamic Apps menu: registry reply-inbox + bundlemgrd
                    // route caps. Order-free since P4a — provisioning this block early
                    // used to shift gpud to 8/9 and kill the present handoff with
                    // `kernel-permission-denied`; both now land where they are declared.
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
                let slots = declared_slots::pin_server_pair(
                    pid,
                    ServiceId::Windowd,
                    window_req,
                    window_rsp,
                )
                .ok_or(InitError::Map("windowd server slots"))?;
                chan.set_send(ServiceId::Windowd, slots.send);
                chan.set_recv(ServiceId::Windowd, slots.recv);
                // gpud may have crashed — a failed pin leaves the route unwired, loudly.
                let (gpud_send_slot, gpud_recv_slot) = declared_slots::pin_route(
                    pid,
                    ServiceId::Windowd,
                    ServiceId::Gpud,
                    gpud_req,
                    gpud_rsp,
                );
                if let (Some(gpud_send), Some(gpud_recv)) = (gpud_send_slot, gpud_recv_slot) {
                    chan.set_send(ServiceId::Gpud, gpud_send);
                    chan.set_recv(ServiceId::Gpud, gpud_recv);
                }
                // Registry reply-inbox + bundlemgrd route (pinned, see the skip path).
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
                    debug_write_hex(slots.recv as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(slots.send as usize);
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
                    let (window_send_slot, window_recv_slot) = declared_slots::pin_route(
                        pid,
                        ServiceId::Inputd,
                        ServiceId::Windowd,
                        window_req,
                        window_rsp,
                    );
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
                let in_slots =
                    declared_slots::pin_server_pair(pid, ServiceId::Inputd, input_req, input_rsp)
                        .ok_or(InitError::Map("inputd server slots"))?;
                let (recv_slot, send_slot) = (in_slots.recv, in_slots.send);
                chan.set_send(ServiceId::Inputd, send_slot);
                chan.set_recv(ServiceId::Inputd, recv_slot);
                let window_send_slot = declared_slots::pin_route_send(
                    pid,
                    ServiceId::Inputd,
                    ServiceId::Windowd,
                    window_req,
                )
                .ok_or(InitError::Map("inputd->windowd send"))?;
                let window_recv_slot = declared_slots::pin_route_recv(
                    pid,
                    ServiceId::Inputd,
                    ServiceId::Windowd,
                    window_rsp,
                )
                .ok_or(InitError::Map("inputd->windowd recv"))?;
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
            // "logd" migrated to the declarative arm below (RFC-0069 batch 4):
            // announce=true keeps its iw-gated slots line + init_caps tally; the
            // generic path is best-effort where the old arm aborted init on a
            // failed transfer — the right semantics for a log sink.
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
                            None => own_id.and_then(|id| {
                                let pair = declared_slots::pin_server_pair(pid, id, req, rsp)?;
                                chan.set_send(id, pair.send);
                                chan.set_recv(id, pair.recv);
                                Some((pair.recv, pair.send))
                            }),
                        };
                        // Push leg (RFC-0075): imed → windowd commit/action
                        // pushes resolve "windowd" by name via this recording.
                        if name == "imed" {
                            provision_imed_legs(pid, eps.imed_osk, window_req, window_rsp, chan);
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

                // RFC-0066/0069: its reply inbox and outbound routes, from its declaration.
                if let Some(spec) = spec {
                    declared_routes::wire_declared_legs(pid, spec, eps, chan);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// `true` if `name` has a bespoke wiring arm in the orchestrator (complex
/// services with routes/reply-inboxes). RFC-0066 Phase 3: services NOT in this set
/// whose `ServiceSpec.exposes_server` is true are provisioned generically from the
/// declarative topology instead of a hand-written arm. As bespoke services are
/// migrated to `ServiceSpec`, they are removed from this set.
pub(crate) fn is_bespoke_wired(name: &str) -> bool {
    matches!(name, "execd" | "hidrawd" | "gpud" | "windowd" | "inputd")
}

/// Provisions a plain server endpoint for a service, driven by the declarative
/// [`crate::service_topology::ServiceSpec`]: both halves of one fresh endpoint land in the
/// service's declared server slots (TASK-0324 P4f-1a). Best-effort: a failure leaves the
/// service unwired rather than aborting init — it must never brick boot.
fn provision_server_endpoint(factory_slot: u32, pid: u32, name: &[u8]) {
    let Some(id) = ServiceId::from_name(name) else {
        return;
    };
    match nexus_abi::ipc_endpoint_create_for(factory_slot, pid, 8) {
        Ok(ep) => {
            let pair = declared_slots::pin_server_pair(pid, id, ep, ep);
            let _ = nexus_abi::cap_close(ep);
            match pair.map(|p| (p.recv, p.send)) {
                Some((recv_slot, send_slot)) => {
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
