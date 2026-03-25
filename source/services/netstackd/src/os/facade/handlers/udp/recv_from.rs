// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: OP_UDP_RECV_FROM handler for netstackd IPC facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_abi::yield_;
use nexus_net::{NetError, UdpSocket as _};

use crate::os::entry_pure::QEMU_USERNET_FALLBACK_IP;
use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::facade::state::{LoopUdp, UdpSock};
use crate::os::facade::udp;
use crate::os::facade::validation;
use crate::os::ipc::handles::UdpId;
use crate::os::ipc::parse::{parse_nonce, parse_u16_le, parse_u32_le};
use crate::os::ipc::reply::{
    reply_status_maybe_nonce, reply_u16_len_ipv4_port_payload_status_maybe_nonce, status_frame,
};
use crate::os::ipc::wire::{
    OP_UDP_RECV_FROM, STATUS_IO, STATUS_MALFORMED, STATUS_NOT_FOUND, STATUS_OK, STATUS_WOULD_BLOCK,
};

pub(crate) fn handle_recv_from<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let udps = &mut ctx.state.udps;

    if validation::validate_exact_or_nonce_len(req.len(), 10).is_malformed() {
        reply(&status_frame(OP_UDP_RECV_FROM, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let nonce = parse_nonce(req, 10);
    let Some(udp_id) = parse_u32_le(req, 4) else {
        reply(&status_frame(OP_UDP_RECV_FROM, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let max = parse_u16_le(req, 8).unwrap_or(0) as usize;
    let max = udp::recv_max_bounded(max);
    let Some(udp_id) = UdpId::from_wire(udp_id) else {
        reply_status_maybe_nonce(reply, OP_UDP_RECV_FROM, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let idx = udp_id.index();
    let Some(Some(sock)) = udps.get(idx) else {
        reply_status_maybe_nonce(reply, OP_UDP_RECV_FROM, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    match sock {
        UdpSock::Udp(_) => {
            let Some(Some(UdpSock::Udp(s))) = udps.get_mut(idx) else {
                reply(&status_frame(OP_UDP_RECV_FROM, STATUS_IO));
                let _ = yield_();
                return DispatchControl::ContinueLoop;
            };
            let mut tmp = [0u8; 460];
            match s.recv_from(&mut tmp[..max]) {
                Ok((n, from)) => {
                    reply_u16_len_ipv4_port_payload_status_maybe_nonce(
                        reply,
                        OP_UDP_RECV_FROM,
                        STATUS_OK,
                        from.ip.0,
                        from.port,
                        &tmp[..n],
                        nonce,
                    );
                }
                Err(NetError::WouldBlock) => {
                    reply_status_maybe_nonce(reply, OP_UDP_RECV_FROM, STATUS_WOULD_BLOCK, nonce);
                }
                Err(_) => {
                    reply_status_maybe_nonce(reply, OP_UDP_RECV_FROM, STATUS_IO, nonce);
                }
            }
        }
        UdpSock::Loop(LoopUdp { rx: _, port: _ }) => {
            let mut tmp = [0u8; 460];
            let Some(Some(UdpSock::Loop(LoopUdp { rx, port }))) = udps.get_mut(idx) else {
                reply(&status_frame(OP_UDP_RECV_FROM, STATUS_IO));
                let _ = yield_();
                return DispatchControl::ContinueLoop;
            };
            let n = rx.pop(&mut tmp[..max]);
            if n == 0 {
                reply_status_maybe_nonce(reply, OP_UDP_RECV_FROM, STATUS_WOULD_BLOCK, nonce);
            } else {
                reply_u16_len_ipv4_port_payload_status_maybe_nonce(
                    reply,
                    OP_UDP_RECV_FROM,
                    STATUS_OK,
                    QEMU_USERNET_FALLBACK_IP,
                    *port,
                    &tmp[..n],
                    nonce,
                );
            }
        }
    }
    DispatchControl::Handled
}
