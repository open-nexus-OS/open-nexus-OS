// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bootstrap subsystem types — CtrlChannel, BootstrapState.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os)
//! ADR: docs/adr/0017-service-architecture.md
//! RFC: docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md

extern crate alloc;
use crate::service_topology::ServiceId;
use alloc::vec::Vec;

/// A requester→target capability route's two slots, each populated independently
/// during wiring. A route is only emitted into the `RouteTable` once BOTH are set.
#[derive(Clone, Copy, Default)]
pub(crate) struct CapPair {
    /// Slot the requester uses to SEND to the target.
    pub send: Option<u32>,
    /// Slot the requester uses to RECV from the target.
    pub recv: Option<u32>,
}

#[derive(Clone, Copy)]
pub(crate) struct CtrlChannel {
    pub svc_name: &'static str,
    pub pid: u32,
    pub ctrl_req_parent_slot: u32,
    pub ctrl_rsp_parent_slot: u32,
    /// CAP_MOVE reply-inbox slots (NOT a route target — shared across this
    /// service's outbound calls), so they stay named rather than in `routes`.
    pub reply_send_slot: Option<u32>,
    pub reply_recv_slot: Option<u32>,
    /// Per-target route slots, indexed by `ServiceId as usize`. Replaces the old
    /// 36 hand-named `<svc>_send_slot`/`<svc>_recv_slot` fields with one uniform
    /// map so adding a service needs no new field (RFC-0061 follow-up, task #100).
    routes: [CapPair; ServiceId::COUNT],
}

impl CtrlChannel {
    /// New channel with the control endpoints set and all routes empty.
    pub fn new(
        svc_name: &'static str,
        pid: u32,
        ctrl_req_parent_slot: u32,
        ctrl_rsp_parent_slot: u32,
    ) -> Self {
        Self {
            svc_name,
            pid,
            ctrl_req_parent_slot,
            ctrl_rsp_parent_slot,
            reply_send_slot: None,
            reply_recv_slot: None,
            routes: [CapPair::default(); ServiceId::COUNT],
        }
    }

    /// Record the SEND slot for the route to `to`.
    pub fn set_send(&mut self, to: ServiceId, slot: u32) {
        self.routes[to as usize].send = Some(slot);
    }

    /// Record the RECV slot for the route to `to`.
    pub fn set_recv(&mut self, to: ServiceId, slot: u32) {
        self.routes[to as usize].recv = Some(slot);
    }

    /// The SEND slot for the route to `to`, if set.
    pub fn send(&self, to: ServiceId) -> Option<u32> {
        self.routes[to as usize].send
    }

    /// The RECV slot for the route to `to`, if set.
    pub fn recv(&self, to: ServiceId) -> Option<u32> {
        self.routes[to as usize].recv
    }

    /// The fully-wired `(send, recv)` route to `to` (both slots set), if any.
    pub fn route(&self, to: ServiceId) -> Option<(u32, u32)> {
        let pair = self.routes[to as usize];
        match (pair.send, pair.recv) {
            (Some(s), Some(r)) => Some((s, r)),
            _ => None,
        }
    }
}

pub(crate) struct BootstrapState {
    pub ctrl_channels: Vec<CtrlChannel>,
    pub route_table: crate::route_table::RouteTable,
    pub pol_ctl_route_req: u32,
    pub pol_ctl_route_rsp: u32,
    pub pol_ctl_exec_req: u32,
    pub pol_ctl_exec_rsp: u32,
    pub upd_req: u32,
    pub upd_reply_send: u32,
    pub upd_reply_recv: u32,
    pub upd_pending: nexus_ipc::reqrep::FrameStash<8, 16>,
}
