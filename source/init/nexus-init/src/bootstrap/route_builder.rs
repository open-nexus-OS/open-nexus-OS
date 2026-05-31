// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Route table builder — extracted from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests in route_table.rs
//! ADR: docs/adr/0017-service-architecture.md
//! RFC: docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md

use crate::bootstrap::CtrlChannel;
use crate::route_table::{CapSlot, RouteTable, ServiceId};
use nexus_abi::Rights;

/// Build a RouteTable from the wired ctrl_channels.
pub(crate) fn build_route_table(channels: &[CtrlChannel]) -> RouteTable {
    let mut table = RouteTable::new();
    for chan in channels {
        let Some(from) = ServiceId::from_name(chan.svc_name.as_bytes()) else {
            continue;
        };
        if let (Some(s), Some(r)) = (chan.vfs_send_slot, chan.vfs_recv_slot) {
            table.add_route(
                from,
                ServiceId::Vfsd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.pkg_send_slot, chan.pkg_recv_slot) {
            table.add_route(
                from,
                ServiceId::Packagefsd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.pol_send_slot, chan.pol_recv_slot) {
            table.add_route(
                from,
                ServiceId::Policyd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.bnd_send_slot, chan.bnd_recv_slot) {
            table.add_route(
                from,
                ServiceId::Bundlemgrd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.upd_send_slot, chan.upd_recv_slot) {
            table.add_route(
                from,
                ServiceId::Updated,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.sam_send_slot, chan.sam_recv_slot) {
            table.add_route(
                from,
                ServiceId::Samgrd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.exe_send_slot, chan.exe_recv_slot) {
            table.add_route(
                from,
                ServiceId::Execd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.key_send_slot, chan.key_recv_slot) {
            table.add_route(
                from,
                ServiceId::Keystored,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.state_send_slot, chan.state_recv_slot) {
            table.add_route(
                from,
                ServiceId::Statefsd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.rng_send_slot, chan.rng_recv_slot) {
            table.add_route(
                from,
                ServiceId::Rngd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.timed_send_slot, chan.timed_recv_slot) {
            table.add_route(
                from,
                ServiceId::Timed,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.window_send_slot, chan.window_recv_slot) {
            table.add_route(
                from,
                ServiceId::Windowd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.input_send_slot, chan.input_recv_slot) {
            table.add_route(
                from,
                ServiceId::Inputd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.fbdev_send_slot, chan.fbdev_recv_slot) {
            table.add_route(
                from,
                ServiceId::Fbdevd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.gpud_send_slot, chan.gpud_recv_slot) {
            table.add_route(
                from,
                ServiceId::Gpud,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.net_send_slot, chan.net_recv_slot) {
            table.add_route(
                from,
                ServiceId::Netstackd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.metrics_send_slot, chan.metrics_recv_slot) {
            table.add_route(
                from,
                ServiceId::Metricsd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.log_send_slot, chan.log_recv_slot) {
            table.add_route(
                from,
                ServiceId::Logd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
        if let (Some(s), Some(r)) = (chan.dsoft_send_slot, chan.dsoft_recv_slot) {
            table.add_route(
                from,
                ServiceId::Dsoftbusd,
                CapSlot::new(s, Rights::SEND),
                CapSlot::new(r, Rights::RECV),
            );
        }
    }
    table
}

/// Send OP_REGISTER to samgrd for every route in the table.
pub(crate) fn populate_samgrd_registry(send_cap: u32, recv_cap: u32, table: &RouteTable) {
    for id in &[
        ServiceId::Vfsd,
        ServiceId::Packagefsd,
        ServiceId::Policyd,
        ServiceId::Bundlemgrd,
        ServiceId::Updated,
        ServiceId::Samgrd,
        ServiceId::Execd,
        ServiceId::Keystored,
        ServiceId::Statefsd,
        ServiceId::Rngd,
        ServiceId::Timed,
        ServiceId::Windowd,
        ServiceId::Inputd,
        ServiceId::Fbdevd,
        ServiceId::Gpud,
        ServiceId::Netstackd,
        ServiceId::Metricsd,
        ServiceId::Logd,
        ServiceId::Dsoftbusd,
        ServiceId::Hidrawd,
        ServiceId::Touchd,
        ServiceId::SelftestClient,
    ] {
        if let Some(route) = table.lookup(*id, *id) {
            let name = id.name();
            let mut req = [0u8; 64];
            req[0] = b'S';
            req[1] = b'M';
            req[2] = 1; // version
            req[3] = 1; // OP_REGISTER
            let name_bytes = name.as_bytes();
            req[4] = name_bytes.len() as u8;
            req[5..9].copy_from_slice(&route.send.slot.to_le_bytes());
            req[9..13].copy_from_slice(&route.recv.slot.to_le_bytes());
            let name_start = 13;
            let name_end = name_start + name_bytes.len();
            req[name_start..name_end].copy_from_slice(name_bytes);
            let req_len = name_end;
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);
            let _ = nexus_abi::ipc_send_v1(
                send_cap,
                &hdr,
                &req[..req_len],
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            );
            let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 16];
            let _ = nexus_abi::ipc_recv_v1(
                recv_cap,
                &mut rh,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            );
        }
    }
}
