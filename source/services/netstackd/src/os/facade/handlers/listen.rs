// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OP_LISTEN handler for netstackd IPC facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_abi::yield_;
use nexus_net::{NetSocketAddrV4, NetStack as _};

use crate::os::config::{LOOPBACK_PORT, LOOPBACK_PORT_B};
use crate::os::entry_pure::QEMU_USERNET_FALLBACK_IP;
use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::facade::state::Listener;
use crate::os::ipc::handles::ListenerId;
use crate::os::ipc::parse::{parse_ipv4_at, parse_nonce, parse_u16_le};
use crate::os::ipc::reply::{reply_status_maybe_nonce, reply_u32_status_maybe_nonce, status_frame};
use crate::os::ipc::wire::{OP_LISTEN, STATUS_IO, STATUS_MALFORMED, STATUS_OK};

pub(crate) fn handle<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let bind_ip = ctx.bind_ip;
    let net = &mut *ctx.net;
    let listeners = &mut ctx.state.listeners;
    let dbg_listen_loopback_logged = &mut ctx.state.dbg_listen_loopback_logged;
    let dbg_listen_tcp_logged = &mut ctx.state.dbg_listen_tcp_logged;

    if req.len() != 6 && req.len() != 10 && req.len() != 14 && req.len() != 18 {
        reply(&status_frame(OP_LISTEN, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let (listen_ip, port, nonce) = if req.len() == 10 || req.len() == 18 {
        let nonce = parse_nonce(req, 10);
        let ip = parse_ipv4_at(req, 4).unwrap_or([0u8; 4]);
        let port = parse_u16_le(req, 8).unwrap_or(0);
        (ip, port, nonce)
    } else {
        let nonce = parse_nonce(req, 6);
        let port = parse_u16_le(req, 4).unwrap_or(0);
        (bind_ip, port, nonce)
    };
    let _ = nexus_abi::debug_println("netstackd: rpc listen");
    if listen_ip == QEMU_USERNET_FALLBACK_IP && (port == LOOPBACK_PORT || port == LOOPBACK_PORT_B) {
        if !*dbg_listen_loopback_logged {
            *dbg_listen_loopback_logged = true;
            // #region agent log
            let _ = nexus_abi::debug_println("dbg:netstackd: listen mode loopback");
            // #endregion
        }
        listeners.push(Some(Listener::Loop { port, pending: None }));
        let id = ListenerId::to_wire(listeners.len() - 1);
        reply_u32_status_maybe_nonce(reply, OP_LISTEN, STATUS_OK, id, nonce);
        let _ = nexus_abi::debug_println("netstackd: rpc listen ok");
    } else {
        if !*dbg_listen_tcp_logged {
            *dbg_listen_tcp_logged = true;
            // #region agent log
            let _ = nexus_abi::debug_println("dbg:netstackd: listen mode tcp");
            // #endregion
        }
        let addr = NetSocketAddrV4::new(listen_ip, port);
        match net.tcp_listen(addr, 1) {
            Ok(l) => {
                listeners.push(Some(Listener::Tcp(l)));
                let id = ListenerId::to_wire(listeners.len() - 1);
                reply_u32_status_maybe_nonce(reply, OP_LISTEN, STATUS_OK, id, nonce);
                let _ = nexus_abi::debug_println("netstackd: rpc listen ok");
            }
            Err(_) => {
                reply_status_maybe_nonce(reply, OP_LISTEN, STATUS_IO, nonce);
                let _ = nexus_abi::debug_println("netstackd: rpc listen FAIL");
            }
        }
    }
    DispatchControl::Handled
}
