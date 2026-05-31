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
use alloc::vec::Vec;

#[derive(Clone, Copy)]
pub(crate) struct CtrlChannel {
    pub svc_name: &'static str, pub pid: u32,
    pub ctrl_req_parent_slot: u32, pub ctrl_rsp_parent_slot: u32,
    pub vfs_send_slot: Option<u32>, pub vfs_recv_slot: Option<u32>,
    pub pkg_send_slot: Option<u32>, pub pkg_recv_slot: Option<u32>,
    pub pol_send_slot: Option<u32>, pub pol_recv_slot: Option<u32>,
    pub bnd_send_slot: Option<u32>, pub bnd_recv_slot: Option<u32>,
    pub upd_send_slot: Option<u32>, pub upd_recv_slot: Option<u32>,
    pub sam_send_slot: Option<u32>, pub sam_recv_slot: Option<u32>,
    pub exe_send_slot: Option<u32>, pub exe_recv_slot: Option<u32>,
    pub key_send_slot: Option<u32>, pub key_recv_slot: Option<u32>,
    pub state_send_slot: Option<u32>, pub state_recv_slot: Option<u32>,
    pub rng_send_slot: Option<u32>, pub rng_recv_slot: Option<u32>,
    pub timed_send_slot: Option<u32>, pub timed_recv_slot: Option<u32>,
    pub window_send_slot: Option<u32>, pub window_recv_slot: Option<u32>,
    pub input_send_slot: Option<u32>, pub input_recv_slot: Option<u32>,
    pub fbdev_send_slot: Option<u32>, pub fbdev_recv_slot: Option<u32>,
    pub gpud_send_slot: Option<u32>, pub gpud_recv_slot: Option<u32>,
    pub net_send_slot: Option<u32>, pub net_recv_slot: Option<u32>,
    pub metrics_send_slot: Option<u32>, pub metrics_recv_slot: Option<u32>,
    pub log_send_slot: Option<u32>, pub log_recv_slot: Option<u32>,
    pub dsoft_send_slot: Option<u32>, pub dsoft_recv_slot: Option<u32>,
    pub reply_send_slot: Option<u32>, pub reply_recv_slot: Option<u32>,
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
