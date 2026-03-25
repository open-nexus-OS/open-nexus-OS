// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OP_ACCEPT handler for netstackd IPC facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_abi::yield_;
use nexus_net::{NetError, TcpListener as _};

use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::facade::state::{Listener, Stream};
use crate::os::facade::tcp;
use crate::os::facade::validation;
use crate::os::ipc::handles::{ListenerId, StreamId};
use crate::os::ipc::parse::{parse_nonce, parse_u32_le};
use crate::os::ipc::reply::{reply_status_maybe_nonce, reply_u32_status_maybe_nonce, status_frame};
use crate::os::ipc::wire::{
    OP_ACCEPT, STATUS_IO, STATUS_MALFORMED, STATUS_NOT_FOUND, STATUS_OK, STATUS_WOULD_BLOCK,
};

pub(crate) fn handle<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let now_ms = ctx.now_ms;
    let net = &mut *ctx.net;
    let listeners = &mut ctx.state.listeners;
    let streams = &mut ctx.state.streams;
    let dbg_accept_status_ok_logged = &mut ctx.state.dbg_accept_status_ok_logged;
    let dbg_accept_status_would_block_logged = &mut ctx.state.dbg_accept_status_would_block_logged;
    let dbg_accept_status_io_logged = &mut ctx.state.dbg_accept_status_io_logged;

    if validation::validate_exact_or_nonce_len(req.len(), 8).is_malformed() {
        reply(&status_frame(OP_ACCEPT, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let nonce = parse_nonce(req, 8);
    let Some(lid_raw) = parse_u32_le(req, 4) else {
        reply(&crate::os::ipc::reply::status_frame(OP_ACCEPT, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let Some(lid) = ListenerId::from_wire(lid_raw) else {
        reply_status_maybe_nonce(reply, OP_ACCEPT, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let Some(Some(l)) = listeners.get_mut(lid.index()) else {
        reply_status_maybe_nonce(reply, OP_ACCEPT, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    match l {
        Listener::Tcp(l) => {
            let accept_result =
                tcp::retry_would_block(net, now_ms, |deadline| l.accept(Some(deadline)));
            match accept_result {
                Ok(s) => {
                    if !*dbg_accept_status_ok_logged {
                        *dbg_accept_status_ok_logged = true;
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:netstackd: accept status ok");
                        // #endregion
                    }
                    streams.push(Some(Stream::TcpAccepted(s)));
                    let sid = StreamId::to_wire(streams.len() - 1);
                    reply_u32_status_maybe_nonce(reply, OP_ACCEPT, STATUS_OK, sid, nonce);
                }
                Err(NetError::WouldBlock) => {
                    if !*dbg_accept_status_would_block_logged {
                        *dbg_accept_status_would_block_logged = true;
                        // #region agent log
                        let _ =
                            nexus_abi::debug_println("dbg:netstackd: accept status would-block");
                        // #endregion
                    }
                    reply_status_maybe_nonce(reply, OP_ACCEPT, STATUS_WOULD_BLOCK, nonce);
                }
                Err(_) => {
                    if !*dbg_accept_status_io_logged {
                        *dbg_accept_status_io_logged = true;
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:netstackd: accept status io");
                        // #endregion
                    }
                    reply_status_maybe_nonce(reply, OP_ACCEPT, STATUS_IO, nonce);
                }
            }
        }
        Listener::Loop { pending, .. } => {
            if let Some(sid) = pending.take() {
                reply_u32_status_maybe_nonce(
                    reply,
                    OP_ACCEPT,
                    STATUS_OK,
                    StreamId::to_wire(sid.index()),
                    nonce,
                );
            } else {
                reply_status_maybe_nonce(reply, OP_ACCEPT, STATUS_WOULD_BLOCK, nonce);
            }
        }
    }
    DispatchControl::Handled
}
