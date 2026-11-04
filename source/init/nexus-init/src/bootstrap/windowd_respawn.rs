// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd's legs and its respawn (RFC-0112, amendment 2026-11-02; TASK-0348 Phase 6).
//! windowd's own legs are its declaration, pinned by ONE path at boot and at a respawn
//! (`wire_windowd_legs` → `declared_routes::wire_declared_legs`): the endpoints come from
//! bootstrap's table at boot and from the handles init keeps of it afterwards
//! (`respawn_plan::LegHandles`). A respawn re-executes the sealed image, mints what died with
//! the old instance — its request (32) and response (8) endpoints, its reply inbox, its timer
//! and its two watch channels (the declared waits) —, pins its legs, drops the answers still
//! queued for the dead instance (gpud's, abilitymgr's: they would be the new one's first
//! frames), re-pins every declared leg INTO windowd (`respawn_plan::client_legs_of`) over the
//! clients' dead capabilities with REPLACE_DEAD, places it on cpu0 like boot, resumes it, and
//! tells execd — the parent of every app — that the compositor is back.
//! OWNERS: @runtime @reliability @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the plan host-tested (`respawn_plan`); the visible restart lane
//!   (`init: service restarted name=windowd`, `windowd: restarted (ghosts=`)
//! RFC: docs/rfcs/RFC-0112-a-services-death-is-a-re-resolve-for-its-clients-the-compositors-restart.md

use crate::bootstrap::declared_routes::{self, LegEndpoints};
use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::helpers::debug_write_bytes;
use crate::bootstrap::respawn::RespawnContext;
use crate::bootstrap::{declared_slots, CtrlChannel};
use crate::os_payload::ENDPOINT_FACTORY_CAP_SLOT;
use crate::respawn_plan::{client_legs_of, LegHandles};
use crate::service_topology::{RouteKind, ServiceId};
use nexus_abi::Rights;

/// windowd's request queue (bootstrap mints the same depth).
const REQUEST_DEPTH: usize = 32;
/// windowd's shared response endpoint.
const RESPONSE_DEPTH: usize = 8;
/// Answers drained from one response endpoint at most (bounded: its queue's depth is far less).
const DRAIN_MAX: usize = 64;

impl LegEndpoints for LegHandles {
    fn minted_reply_ep(&self, _id: ServiceId) -> Option<u32> {
        None
    }
    fn server_pair(&self, id: ServiceId) -> Option<(u32, u32)> {
        self.pair(id)
    }
    fn request_ep(&self, _from: ServiceId, to: ServiceId) -> Option<u32> {
        self.request(to)
    }
}

/// What init keeps of bootstrap for windowd's respawn: the endpoints its legs are pinned from
/// and execd's request endpoint (the compositor-back notice).
#[derive(Clone, Copy)]
pub(crate) struct WindowdKept {
    legs: LegHandles,
    execd_req: Option<u32>,
}

impl WindowdKept {
    /// Kept from bootstrap's table — init keeps every one of these handles for the boot (none
    /// is in `close_wired_eps`).
    pub(crate) fn of(eps: &Endpoints) -> Option<Self> {
        let spec = crate::service_topology::spec_for(b"windowd")?;
        let mut legs = LegHandles::default();
        for route in spec.routes_to {
            match route.kind {
                RouteKind::SharedResponse => legs.keep_pair(route.to, eps.server_pair(route.to)),
                RouteKind::ReplyInbox | RouteKind::PrivateInbox { .. } => {
                    legs.keep_request(route.to, eps.request_ep(ServiceId::Windowd, route.to));
                }
            }
        }
        let execd_req = eps.server_pair(ServiceId::Execd).map(|(req, _)| req);
        Some(Self { legs, execd_req })
    }
}

/// Pins windowd's declared legs into `pid` — at boot from bootstrap's table, at a respawn from
/// the kept handles: one path (RFC-0112). Every leg is in windowd's `ServiceSpec`.
pub(crate) fn wire_windowd_legs(pid: u32, legs: &impl LegEndpoints, chan: &mut CtrlChannel) {
    if let Some(spec) = crate::service_topology::spec_for(b"windowd") {
        declared_routes::wire_declared_legs(pid, spec, legs, chan);
    }
}

/// Respawns windowd from boot-held state; the new pid, or `None` (named by the caller).
pub(crate) fn respawn_windowd(
    ctx: &RespawnContext,
    kept: &WindowdKept,
    persist: Option<&mut crate::bootstrap::persist::SupervisionPersist>,
    channels: &mut [CtrlChannel],
) -> Option<u32> {
    let (image, stack_pages, global_pointer, name) = ctx.image_for("windowd")?;
    let at = channels.iter().position(|c| c.svc_name == "windowd")?;
    let pid = nexus_abi::exec_vmo(image, stack_pages, global_pointer, name, false).ok()?;
    let (ctrl_req, ctrl_rsp) =
        (channels[at].ctrl_req_parent_slot, channels[at].ctrl_rsp_parent_slot);
    if !crate::bootstrap::respawn::reattach_ctrl(pid, (ctrl_req, ctrl_rsp), ctx.stage_fence) {
        return None;
    }
    // What died with the old instance, minted for the new one.
    let req =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, REQUEST_DEPTH).ok()?;
    let rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, RESPONSE_DEPTH)
        .ok()
        .or_else(|| {
            let _ = nexus_abi::cap_close(req);
            None
        })?;
    let mut chan = CtrlChannel::new("windowd", pid, ctrl_req, ctrl_rsp);
    let wired = declared_slots::pin_server_pair(pid, ServiceId::Windowd, req, rsp).map(|slots| {
        chan.set_recv(ServiceId::Windowd, slots.recv);
        chan.set_send(ServiceId::Windowd, slots.send);
        declared_routes::pin_declared_waits(pid, ServiceId::Windowd);
        wire_windowd_legs(pid, &kept.legs, &mut chan);
    });
    // The answers the dead instance never read would be the new one's first frames.
    for target in [ServiceId::Gpud, ServiceId::Abilitymgr] {
        if let Some((_, response)) = kept.legs.pair(target) {
            drain_stale(response);
        }
    }
    // Every declared leg into windowd, over its client's dead capability.
    let repinned = if wired.is_some() { repin_clients(channels, req, rsp) } else { 0 };
    let _ = nexus_abi::cap_close(req);
    let _ = nexus_abi::cap_close(rsp);
    wired?;
    crate::bootstrap::resume::apply_affinity("windowd", pid);
    crate::bootstrap::resume::apply_qos("windowd", pid);
    if let Some(persist) = persist {
        persist.bump_restart_count(name);
    }
    if nexus_abi::task_resume(pid).is_err() {
        debug_write_bytes(b"init: FAIL respawn resume\n");
        return None;
    }
    channels[at] = chan;
    crate::bootstrap::diag::emit_marker_atomic(
        &[b"init: windowd re-pinned (clients=0x"],
        Some(repinned as u64),
    );
    notify_execd(kept.execd_req);
    Some(pid)
}

/// Re-pins every declared leg into windowd over its client's dead capability (REPLACE_DEAD):
/// the SEND half on `req`, a `SharedResponse` leg's RECV half on `rsp`. Returns the legs
/// re-pinned; a client not running (not in the image) has nothing to re-pin.
fn repin_clients(channels: &[CtrlChannel], req: u32, rsp: u32) -> usize {
    let mut repinned = 0;
    for leg in client_legs_of(ServiceId::Windowd) {
        let Some(client) = channels.iter().find(|c| {
            crate::service_topology::ServiceId::from_name(c.svc_name.as_bytes()) == Some(leg.client)
        }) else {
            continue;
        };
        let pin = |cap: u32, rights: Rights, slot: u32| {
            nexus_abi::cap_transfer_to_slot_replace_dead(client.pid, cap, rights, slot).is_ok()
        };
        let send = pin(req, Rights::SEND, leg.slots.send);
        let recv = !leg.takes_response() || pin(rsp, Rights::RECV, leg.slots.recv);
        if send && recv {
            repinned += 1;
        } else {
            crate::bootstrap::diag::emit_marker_atomic(
                &[b"init: FAIL windowd re-pin client=", client.svc_name.as_bytes()],
                None,
            );
        }
    }
    repinned
}

/// Drops what waits on a response endpoint init reads with windowd (no frame of it is init's);
/// a capability a dropped answer moved goes with it.
fn drain_stale(response: u32) {
    let mut buf = [0u8; 64];
    for _ in 0..DRAIN_MAX {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let flags = nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE;
        if nexus_abi::ipc_recv_v1(response, &mut hdr, &mut buf, flags, 0).is_err() {
            return;
        }
        if hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE != 0 {
            let _ = nexus_abi::cap_close(hdr.src);
        }
    }
}

/// Tells execd the compositor is back — one-way (`nexus_wire::execd::OP_COMPOSITOR_BACK`).
fn notify_execd(execd_req: Option<u32>) {
    let Some(slot) = execd_req else { return };
    let frame = nexus_wire::execd::encode_compositor_back();
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    if nexus_abi::ipc_send_v1(slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0).is_err() {
        debug_write_bytes(b"init: FAIL compositor back to execd (send)\n");
    }
}
