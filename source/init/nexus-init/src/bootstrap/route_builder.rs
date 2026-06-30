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

/// Build a RouteTable from the wired ctrl_channels — one entry per fully-wired
/// `(send, recv)` route in each channel's per-`ServiceId` routing map.
pub(crate) fn build_route_table(channels: &[CtrlChannel]) -> RouteTable {
    let mut table = RouteTable::new();
    for chan in channels {
        let Some(from) = ServiceId::from_name(chan.svc_name.as_bytes()) else {
            continue;
        };
        for &to in &ServiceId::ALL {
            if let Some((send, recv)) = chan.route(to) {
                table.add_route(
                    from,
                    to,
                    CapSlot::new(send, Rights::SEND),
                    CapSlot::new(recv, Rights::RECV),
                );
            }
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
