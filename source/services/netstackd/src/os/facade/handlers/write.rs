// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OP_WRITE handler for netstackd IPC facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_abi::yield_;
use nexus_net::{NetError, TcpStream as _};

use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::facade::state::Stream;
use crate::os::facade::tcp;
use crate::os::facade::validation;
use crate::os::ipc::handles::StreamId;
use crate::os::ipc::parse::{parse_nonce, parse_u32_le};
use crate::os::ipc::reply::{
    reply_status_maybe_nonce, reply_u16_field_status_maybe_nonce, status_frame,
};
use crate::os::ipc::wire::{
    OP_WRITE, STATUS_IO, STATUS_MALFORMED, STATUS_NOT_FOUND, STATUS_OK, STATUS_WOULD_BLOCK,
};
use crate::os::loopback::reject_oversized_loopback_payload;

pub(crate) fn handle<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let now_ms = ctx.now_ms;
    let net = &mut *ctx.net;
    let streams = &mut ctx.state.streams;

    if req.len() < 10 {
        reply(&status_frame(OP_WRITE, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let Some(sid) = parse_u32_le(req, 4) else {
        reply(&status_frame(OP_WRITE, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let len = u16::from_le_bytes([req[8], req[9]]) as usize;
    let nonce = parse_nonce(req, 10 + len);
    if validation::validate_payload_len(req.len(), 10, len).is_malformed() {
        reply(&status_frame(OP_WRITE, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let Some(sid) = StreamId::from_wire(sid) else {
        reply_status_maybe_nonce(reply, OP_WRITE, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let sid0 = sid.index();
    let Some(Some(kind)) = streams.get(sid0) else {
        reply_status_maybe_nonce(reply, OP_WRITE, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    match kind {
        Stream::TcpDial(_) | Stream::TcpAccepted(_) => {
            let Some(Some(Stream::TcpDial(s) | Stream::TcpAccepted(s))) = streams.get_mut(sid0)
            else {
                reply(&status_frame(OP_WRITE, STATUS_IO));
                let _ = yield_();
                return DispatchControl::ContinueLoop;
            };
            let write_result = tcp::retry_would_block(net, now_ms, |deadline| {
                s.write(Some(deadline), &req[10..10 + len])
            });
            match write_result {
                Ok(n) => {
                    if n == 0 {
                        reply_status_maybe_nonce(reply, OP_WRITE, STATUS_WOULD_BLOCK, nonce);
                    } else {
                        reply_u16_field_status_maybe_nonce(
                            reply, OP_WRITE, STATUS_OK, n as u16, nonce,
                        );
                    }
                }
                Err(NetError::WouldBlock) => {
                    reply_status_maybe_nonce(reply, OP_WRITE, STATUS_WOULD_BLOCK, nonce);
                }
                Err(_) => {
                    reply_status_maybe_nonce(reply, OP_WRITE, STATUS_IO, nonce);
                }
            }
        }
        Stream::Loop { peer, .. } => {
            let peer0 = peer.index();
            let Some(Some(Stream::Loop { rx, .. })) = streams.get_mut(peer0) else {
                reply(&status_frame(OP_WRITE, STATUS_IO));
                let _ = yield_();
                return DispatchControl::ContinueLoop;
            };
            if reject_oversized_loopback_payload(len) {
                reply_status_maybe_nonce(reply, OP_WRITE, STATUS_IO, nonce);
                let _ = yield_();
                return DispatchControl::ContinueLoop;
            }
            let wrote = rx.push(&req[10..10 + len]);
            if wrote == 0 {
                reply_status_maybe_nonce(reply, OP_WRITE, STATUS_WOULD_BLOCK, nonce);
            } else {
                reply_u16_field_status_maybe_nonce(reply, OP_WRITE, STATUS_OK, wrote as u16, nonce);
            }
        }
    }
    DispatchControl::Handled
}
