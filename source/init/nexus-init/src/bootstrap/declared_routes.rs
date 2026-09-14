// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ONE provisioning path for a service's declared outbound legs (TASK-0324 P4f-5) —
//! its CAP_MOVE reply inbox and every `routes_to` entry, each pinned into the slot the topology
//! declares and recorded for the route responder. The generic wiring arm runs it for every
//! server it provisions; the pre-resume pass runs it for the proof harness, which runs from
//! wave 1 on and must hold its legs before it first runs (RFC-0093 §4).
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P4)
//! API_STABILITY: Internal
//! TEST_COVERAGE: `nexus-service-topology` slot tests + every QEMU lane (a mis-pinned leg is
//! `init: FAIL declared slot …`, not a silent fallback)

use crate::bootstrap::declared_slots;
use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::ENDPOINT_FACTORY_CAP_SLOT;
use crate::service_topology::{RouteKind, ServiceId, ServiceSpec};
use nexus_abi::Rights;

/// Pins `spec`'s reply inbox and outbound routes into `pid` and records them on `chan`.
/// Best-effort: a leg whose pin fails is reported by the pin and left unwired, never bricks boot.
pub(crate) fn wire_declared_legs(
    pid: u32,
    spec: &ServiceSpec,
    eps: &Endpoints,
    chan: &mut CtrlChannel,
) {
    let name = chan.svc_name;
    // CAP_MOVE reply inbox: PRE-MINTED when bootstrap made one, freshly created otherwise. Same
    // lifecycle either way: pin RECV+SEND, close the init-side slot.
    let mut reply_recv_opt: Option<u32> = None;
    if spec.reply_inbox {
        let inbox_ep = eps
            .minted_reply_ep(spec.id)
            .or_else(|| nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8).ok());
        if let Some(reply_ep) = inbox_ep {
            let inbox = declared_slots::pin_reply_inbox(pid, spec.id, reply_ep);
            let _ = nexus_abi::cap_close(reply_ep);
            if let Some(inbox) = inbox {
                chan.reply_recv_slot = Some(inbox.recv);
                chan.reply_send_slot = Some(inbox.send);
                reply_recv_opt = Some(inbox.recv);
            }
        }
    }
    for route in spec.routes_to {
        match route.kind {
            // Replies arrive on the TARGET's pre-minted response endpoint, shared directly
            // (vfsd → packagefsd).
            RouteKind::SharedResponse => {
                if let Some((t_req, t_rsp)) = eps.server_pair(route.to) {
                    let s = declared_slots::pin_route_send(pid, spec.id, route.to, t_req);
                    let r = declared_slots::pin_route_recv(pid, spec.id, route.to, t_rsp);
                    if let (Some(s), Some(r)) = (s, r) {
                        chan.set_send(route.to, s);
                        chan.set_recv(route.to, r);
                        if spec.announce {
                            announce_route_ok(name, route.to);
                        }
                    }
                }
            }
            // Replies arrive on an inbox PRIVATE to this route (imed's settingsd and statefsd
            // legs): the request SEND and both halves of a fresh inbox land in the declared
            // slots (TASK-0324 P4f-1b).
            RouteKind::PrivateInbox { .. } => {
                if let Some((t_req, _)) = eps.server_pair(route.to) {
                    if let Some(pair) = declared_slots::pin_private_inbox_route(
                        pid,
                        spec.id,
                        route.to,
                        t_req,
                        ENDPOINT_FACTORY_CAP_SLOT,
                    ) {
                        chan.set_send(route.to, pair.send);
                        chan.set_recv(route.to, pair.recv);
                        if spec.announce || crate::bootstrap::diag::raw_or_expanded(name) {
                            announce_route_ok(name, route.to);
                        }
                    }
                }
            }
            // Replies arrive on this service's CAP_MOVE inbox; the request endpoint comes from
            // the one table of minted endpoints (TASK-0324 P4f-4/P4f-5).
            RouteKind::ReplyInbox => {
                let (Some(reply_recv), Some(t_req)) =
                    (reply_recv_opt, eps.request_ep(spec.id, route.to))
                else {
                    continue;
                };
                if let Some(s) = declared_slots::pin_route_send(pid, spec.id, route.to, t_req) {
                    chan.set_send(route.to, s);
                    chan.set_recv(route.to, reply_recv);
                    if spec.announce {
                        announce_route_ok(name, route.to);
                    }
                }
            }
        }
    }
}

/// TASK-0324 P7-d: a declared timer-notify endpoint — pacing without a recv timeout. Both
/// halves land in the service's declared slots; it binds its kernel timer cap to the SEND half
/// and waits on the RECV half as a waitset member. Runs for bespoke and generic arms alike.
pub(crate) fn pin_timer_notify(pid: u32, svc: ServiceId, eps: &Endpoints) {
    use crate::service_topology::NamedSlot;
    let Some(ep) = eps.timer_notify_ep(svc) else {
        return;
    };
    let recv = declared_slots::pin_named(pid, svc, NamedSlot::TimerNotifyRecv, ep, Rights::RECV);
    let send = declared_slots::pin_named(pid, svc, NamedSlot::TimerNotifySend, ep, Rights::SEND);
    if recv.is_none() || send.is_none() {
        crate::bootstrap::diag::emit_marker_atomic(
            &[b"init: timer-notify FAIL svc=", svc.name().as_bytes()],
            None,
        );
    }
}

/// The proof harness's legs, pinned BEFORE wave 1 resumes it (TASK-0324 P4f-5). It used to be
/// wired by transfer order in the middle of `wire_services`, while it was already running — the
/// order was the contract, and a separate post-wiring pass existed only so that one more leg
/// would not shift the numbers the harness hardcoded.
pub(crate) fn wire_proof_harness(ctrls: &mut [CtrlChannel], eps: &Endpoints) {
    let Some(spec) = crate::service_topology::spec_for(b"selftest-client") else {
        return;
    };
    for chan in ctrls.iter_mut().filter(|chan| chan.svc_name == "selftest-client") {
        let pid = chan.pid;
        wire_declared_legs(pid, spec, eps, chan);
    }
}

/// `init: <svc> route-><target> ok` — the one announce line for a provisioned route. ONE atomic
/// line: it is a ladder witness (`init: netstackd route->policyd ok`, TASK-0324 P4f-4), and
/// byte-wise writes tear against the services already running during wiring.
fn announce_route_ok(name: &str, to: ServiceId) {
    crate::bootstrap::diag::emit_marker_atomic(
        &[b"init: ", name.as_bytes(), b" route->", to.name().as_bytes(), b" ok"],
        None,
    );
}
