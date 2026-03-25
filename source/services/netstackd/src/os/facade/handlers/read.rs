// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OP_READ handler for netstackd IPC facade
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
use crate::os::ipc::parse::{parse_nonce, parse_u16_le, parse_u32_le};
use crate::os::ipc::reply::{
    reply_status_maybe_nonce, reply_u16_len_payload_status_maybe_nonce, status_frame,
};
use crate::os::ipc::wire::{
    OP_READ, STATUS_IO, STATUS_MALFORMED, STATUS_NOT_FOUND, STATUS_OK, STATUS_WOULD_BLOCK,
};

pub(crate) fn handle<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let now_ms = ctx.now_ms;
    let net = &mut *ctx.net;
    let streams = &mut ctx.state.streams;

    if validation::validate_exact_or_nonce_len(req.len(), 10).is_malformed() {
        reply(&status_frame(OP_READ, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let nonce = parse_nonce(req, 10);
    let Some(sid) = parse_u32_le(req, 4) else {
        reply(&status_frame(OP_READ, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let max = parse_u16_le(req, 8).unwrap_or(0) as usize;
    let max = core::cmp::min(max, 480);
    let Some(sid) = StreamId::from_wire(sid) else {
        reply_status_maybe_nonce(reply, OP_READ, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let Some(Some(s)) = streams.get_mut(sid.index()) else {
        reply_status_maybe_nonce(reply, OP_READ, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    match s {
        Stream::TcpDial(s) | Stream::TcpAccepted(s) => {
            let mut buf = [0u8; 480];
            let read_result = tcp::retry_would_block(net, now_ms, |deadline| {
                s.read(Some(deadline), &mut buf[..max])
            });
            match read_result {
                Ok(n) => {
                    reply_u16_len_payload_status_maybe_nonce(
                        reply,
                        OP_READ,
                        STATUS_OK,
                        &buf[..n],
                        nonce,
                    );
                }
                Err(NetError::WouldBlock) => {
                    reply_status_maybe_nonce(reply, OP_READ, STATUS_WOULD_BLOCK, nonce);
                }
                Err(_) => {
                    reply_status_maybe_nonce(reply, OP_READ, STATUS_IO, nonce);
                }
            }
        }
        Stream::Loop { rx, .. } => {
            let mut out = [0u8; 480];
            let n = rx.pop(&mut out[..max]);
            if n == 0 {
                reply_status_maybe_nonce(reply, OP_READ, STATUS_WOULD_BLOCK, nonce);
            } else {
                reply_u16_len_payload_status_maybe_nonce(
                    reply,
                    OP_READ,
                    STATUS_OK,
                    &out[..n],
                    nonce,
                );
            }
        }
    }
    DispatchControl::Handled
}
