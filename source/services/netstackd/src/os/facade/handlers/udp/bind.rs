// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: OP_UDP_BIND handler for netstackd IPC facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_abi::yield_;
use nexus_net::{NetError, NetSocketAddrV4, NetStack as _};

use crate::os::config::{LOOPBACK_PORT, LOOPBACK_UDP_PORT, LOOPBACK_UDP_QUIC_CLIENT_PORT};
use crate::os::entry_pure::QEMU_USERNET_FALLBACK_IP;
use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::facade::state::{LoopUdp, UdpSock};
use crate::os::ipc::handles::UdpId;
use crate::os::ipc::parse::{parse_ipv4_at, parse_nonce, parse_u16_le};
use crate::os::ipc::reply::{reply_status_maybe_nonce, reply_u32_status_maybe_nonce, status_frame};
use crate::os::ipc::wire::{OP_UDP_BIND, STATUS_IO, STATUS_MALFORMED, STATUS_OK};
use crate::os::loopback::LoopBuf;

pub(crate) fn handle_bind<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let ctx_bind_ip = ctx.bind_ip;
    let net = &mut *ctx.net;
    let udps = &mut ctx.state.udps;
    let dbg_udp_bind_logged = &mut ctx.state.dbg_udp_bind_logged;
    let reply_slot = ctx.reply_slot;

    if !*dbg_udp_bind_logged {
        *dbg_udp_bind_logged = true;
        let _ = nexus_abi::debug_println("netstackd: rpc udp bind");
        if reply_slot.is_none() {
            let _ = nexus_abi::debug_println("netstackd: udp bind missing reply cap");
        }
    }
    if req.len() != 6 && req.len() != 10 && req.len() != 14 && req.len() != 18 {
        reply(&status_frame(OP_UDP_BIND, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let (bind_ip, port, nonce) = if req.len() == 10 || req.len() == 18 {
        let nonce = parse_nonce(req, 10);
        let ip = parse_ipv4_at(req, 4).unwrap_or([0u8; 4]);
        let port = parse_u16_le(req, 8).unwrap_or(0);
        (ip, port, nonce)
    } else {
        let nonce = parse_nonce(req, 6);
        (ctx_bind_ip, parse_u16_le(req, 4).unwrap_or(0), nonce)
    };
    if (port == LOOPBACK_UDP_PORT || port == LOOPBACK_PORT || port == LOOPBACK_UDP_QUIC_CLIENT_PORT)
        && (bind_ip == QEMU_USERNET_FALLBACK_IP || bind_ip == [0, 0, 0, 0])
    {
        udps.push(Some(UdpSock::Loop(LoopUdp { rx: LoopBuf::new(), port, last_from_port: 0 })));
        let id = UdpId::to_wire(udps.len() - 1);
        reply_u32_status_maybe_nonce(reply, OP_UDP_BIND, STATUS_OK, id, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let addr = NetSocketAddrV4::new(bind_ip, port);
    match net.udp_bind(addr) {
        Ok(s) => {
            udps.push(Some(UdpSock::Udp(s)));
            let id = UdpId::to_wire(udps.len() - 1);
            reply_u32_status_maybe_nonce(reply, OP_UDP_BIND, STATUS_OK, id, nonce);
        }
        Err(NetError::AddrInUse) => {
            reply_status_maybe_nonce(reply, OP_UDP_BIND, STATUS_IO, nonce);
        }
        Err(_) => {
            reply_status_maybe_nonce(reply, OP_UDP_BIND, STATUS_IO, nonce);
        }
    }
    DispatchControl::Handled
}
