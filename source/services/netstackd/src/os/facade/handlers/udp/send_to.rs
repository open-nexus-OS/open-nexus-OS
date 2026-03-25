// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: OP_UDP_SEND_TO handler for netstackd IPC facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_abi::yield_;
use nexus_net::{NetError, NetSocketAddrV4, UdpSocket as _};

use crate::os::entry_pure::QEMU_USERNET_FALLBACK_IP;
use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::facade::state::{LoopUdp, UdpSock};
use crate::os::facade::validation;
use crate::os::ipc::handles::UdpId;
use crate::os::ipc::parse::{parse_ipv4_at, parse_nonce, parse_u16_le, parse_u32_le};
use crate::os::ipc::reply::{
    reply_status_maybe_nonce, reply_u16_field_status_maybe_nonce, status_frame,
};
use crate::os::ipc::wire::{
    OP_UDP_SEND_TO, STATUS_IO, STATUS_MALFORMED, STATUS_NOT_FOUND, STATUS_OK, STATUS_WOULD_BLOCK,
};

pub(crate) fn handle_send_to<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let udps = &mut ctx.state.udps;

    if req.len() < 4 + 4 + 4 + 2 + 2 {
        reply(&status_frame(OP_UDP_SEND_TO, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let Some(udp_id) = parse_u32_le(req, 4) else {
        reply(&status_frame(OP_UDP_SEND_TO, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let ip = parse_ipv4_at(req, 8).unwrap_or([0u8; 4]);
    let port = parse_u16_le(req, 12).unwrap_or(0);
    let len = parse_u16_le(req, 14).unwrap_or(0) as usize;
    let nonce = parse_nonce(req, 16 + len);
    if validation::validate_payload_len(req.len(), 16, len).is_malformed() {
        reply(&status_frame(OP_UDP_SEND_TO, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let Some(udp_id) = UdpId::from_wire(udp_id) else {
        reply_status_maybe_nonce(reply, OP_UDP_SEND_TO, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let idx = udp_id.index();
    let Some(Some(sock)) = udps.get(idx) else {
        reply_status_maybe_nonce(reply, OP_UDP_SEND_TO, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    match sock {
        UdpSock::Udp(_) => {
            let Some(Some(UdpSock::Udp(s))) = udps.get_mut(idx) else {
                reply_status_maybe_nonce(reply, OP_UDP_SEND_TO, STATUS_IO, nonce);
                let _ = yield_();
                return DispatchControl::ContinueLoop;
            };
            let dst = NetSocketAddrV4::new(ip, port);
            match s.send_to(&req[16..16 + len], dst) {
                Ok(n) => {
                    reply_u16_field_status_maybe_nonce(
                        reply,
                        OP_UDP_SEND_TO,
                        STATUS_OK,
                        n as u16,
                        nonce,
                    );
                }
                Err(NetError::WouldBlock) => {
                    reply_status_maybe_nonce(reply, OP_UDP_SEND_TO, STATUS_WOULD_BLOCK, nonce);
                }
                Err(_) => {
                    reply_status_maybe_nonce(reply, OP_UDP_SEND_TO, STATUS_IO, nonce);
                }
            }
        }
        UdpSock::Loop(LoopUdp { rx: _, port: local }) => {
            if ip != QEMU_USERNET_FALLBACK_IP || port != *local {
                reply_status_maybe_nonce(reply, OP_UDP_SEND_TO, STATUS_IO, nonce);
                let _ = yield_();
                return DispatchControl::ContinueLoop;
            }
            let Some(Some(UdpSock::Loop(LoopUdp { rx, .. }))) = udps.get_mut(idx) else {
                reply(&status_frame(OP_UDP_SEND_TO, STATUS_IO));
                let _ = yield_();
                return DispatchControl::ContinueLoop;
            };
            let wrote = rx.push(&req[16..16 + len]);
            if wrote == 0 {
                reply_status_maybe_nonce(reply, OP_UDP_SEND_TO, STATUS_WOULD_BLOCK, nonce);
            } else {
                reply_u16_field_status_maybe_nonce(
                    reply,
                    OP_UDP_SEND_TO,
                    STATUS_OK,
                    wrote as u16,
                    nonce,
                );
            }
        }
    }
    DispatchControl::Handled
}
