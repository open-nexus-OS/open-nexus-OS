// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OP_CONNECT handler for netstackd IPC facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_abi::yield_;
use nexus_net::{NetError, NetSocketAddrV4, NetStack as _};

use crate::os::config::{LOOPBACK_PORT, LOOPBACK_PORT_B, TCP_READY_SPIN_BUDGET, TCP_READY_STEP_MS};
use crate::os::entry_pure::{is_qemu_loopback_target, QEMU_USERNET_FALLBACK_IP};
use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::facade::ops;
use crate::os::facade::state::{Listener, Stream};
use crate::os::facade::validation;
use crate::os::ipc::handles::StreamId;
use crate::os::ipc::parse::{parse_ipv4_at, parse_nonce, parse_u16_le};
use crate::os::ipc::reply::{reply_status_maybe_nonce, reply_u32_status_maybe_nonce, status_frame};
use crate::os::ipc::wire::{
    OP_CONNECT, STATUS_IO, STATUS_MALFORMED, STATUS_OK, STATUS_WOULD_BLOCK,
};
use crate::os::loopback::LoopBuf;

pub(crate) fn handle<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let now_ms = ctx.now_ms;
    let net = &mut *ctx.net;
    let listeners = &mut ctx.state.listeners;
    let streams = &mut ctx.state.streams;
    let pending_dial = &mut ctx.state.pending_dial;
    let dbg_connect_target_printed = &mut ctx.state.dbg_connect_target_printed;
    let dbg_loopback_connect_logged = &mut ctx.state.dbg_loopback_connect_logged;
    let dbg_connect_kick_ok_logged = &mut ctx.state.dbg_connect_kick_ok_logged;
    let dbg_connect_kick_would_block_logged = &mut ctx.state.dbg_connect_kick_would_block_logged;
    let dbg_connect_pending_set_logged = &mut ctx.state.dbg_connect_pending_set_logged;
    let dbg_connect_pending_reused_logged = &mut ctx.state.dbg_connect_pending_reused_logged;
    let dbg_connect_pending_stale_logged = &mut ctx.state.dbg_connect_pending_stale_logged;
    let dbg_connect_status_would_block_logged =
        &mut ctx.state.dbg_connect_status_would_block_logged;
    let dbg_connect_status_io_logged = &mut ctx.state.dbg_connect_status_io_logged;

    if validation::validate_exact_or_nonce_len(req.len(), 10).is_malformed() {
        reply(&status_frame(OP_CONNECT, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let nonce = parse_nonce(req, 10);
    let ip = parse_ipv4_at(req, 4).unwrap_or([0u8; 4]);
    let port = parse_u16_le(req, 8).unwrap_or(0);
    ctx.state.dbg_connect_req_count = ctx.state.dbg_connect_req_count.wrapping_add(1);
    if ctx.state.dbg_connect_req_count == 1 {
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:netstackd: connect req count 1");
        // #endregion
    } else if ctx.state.dbg_connect_req_count == 512 {
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:netstackd: connect req count 512");
        // #endregion
    } else if ctx.state.dbg_connect_req_count == 4096 {
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:netstackd: connect req count 4096");
        // #endregion
    }
    if !*dbg_connect_target_printed
        && ip != QEMU_USERNET_FALLBACK_IP
        && (port == 34_567 || port == 34_568)
    {
        *dbg_connect_target_printed = true;
    }
    if is_qemu_loopback_target(ip, port, LOOPBACK_PORT, LOOPBACK_PORT_B) {
        let a = StreamId::from_index(streams.len());
        let b = StreamId::from_index(streams.len() + 1);
        streams.push(Some(Stream::Loop { peer: b, rx: LoopBuf::new() }));
        streams.push(Some(Stream::Loop { peer: a, rx: LoopBuf::new() }));
        for l in listeners.iter_mut() {
            if let Some(Listener::Loop { port: listen_port, pending }) = l {
                if *listen_port == port && pending.is_none() {
                    *pending = Some(b);
                    break;
                }
            }
        }
        reply_u32_status_maybe_nonce(
            reply,
            OP_CONNECT,
            STATUS_OK,
            StreamId::to_wire(a.index()),
            nonce,
        );
        if !*dbg_loopback_connect_logged {
            *dbg_loopback_connect_logged = true;
            let _ = nexus_abi::debug_println("netstackd: rpc connect loopback ok");
        }
    } else {
        let remote = NetSocketAddrV4::new(ip, port);

        let mut reused_pending = false;
        let mut drop_stale_pending = false;
        if let Some((pending_remote, pending_stream)) = pending_dial.as_mut() {
            if pending_remote.ip == remote.ip && pending_remote.port == remote.port {
                if pending_stream.is_closed_or_listen() {
                    drop_stale_pending = true;
                } else {
                    reused_pending = true;
                    if !*dbg_connect_pending_reused_logged {
                        *dbg_connect_pending_reused_logged = true;
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:netstackd: connect pending reused");
                        // #endregion
                    }
                    if pending_stream.wait_writable_bounded(TCP_READY_SPIN_BUDGET) {
                        if !*dbg_connect_kick_ok_logged {
                            *dbg_connect_kick_ok_logged = true;
                            // #region agent log
                            let _ = nexus_abi::debug_println("dbg:netstackd: connect kick ok");
                            // #endregion
                        }
                        let Some((_, stream)) = pending_dial.take() else {
                            if ops::is_unexpected_pending_connect_state(reused_pending, true) {
                                let _ = nexus_abi::debug_println(
                                    "netstackd: connect pending state unexpected",
                                );
                            }
                            reply(&status_frame(OP_CONNECT, STATUS_IO));
                            let _ = yield_();
                            return DispatchControl::ContinueLoop;
                        };
                        streams.push(Some(Stream::TcpDial(stream)));
                        let sid = StreamId::to_wire(streams.len() - 1);
                        reply_u32_status_maybe_nonce(reply, OP_CONNECT, STATUS_OK, sid, nonce);
                        let _ = nexus_abi::debug_println("netstackd: rpc connect ok");
                    } else {
                        if !*dbg_connect_kick_would_block_logged {
                            *dbg_connect_kick_would_block_logged = true;
                            // #region agent log
                            let _ =
                                nexus_abi::debug_println("dbg:netstackd: connect kick would-block");
                            // #endregion
                        }
                        reply_status_maybe_nonce(reply, OP_CONNECT, STATUS_WOULD_BLOCK, nonce);
                    }
                }
            } else {
                if let Some((_, stale)) = pending_dial.take() {
                    stale.close_and_remove();
                }
            }
        }
        if drop_stale_pending {
            if !*dbg_connect_pending_stale_logged {
                *dbg_connect_pending_stale_logged = true;
                // #region agent log
                let _ = nexus_abi::debug_println("dbg:netstackd: connect pending stale");
                // #endregion
            }
            if let Some((_, stale)) = pending_dial.take() {
                stale.close_and_remove();
            }
        }
        if reused_pending {
            let _ = yield_();
            return DispatchControl::ContinueLoop;
        }

        match net.tcp_connect(remote, Some(now_ms + TCP_READY_STEP_MS)) {
            Ok(mut s) => {
                if s.wait_writable_bounded(TCP_READY_SPIN_BUDGET) {
                    if !*dbg_connect_kick_ok_logged {
                        *dbg_connect_kick_ok_logged = true;
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:netstackd: connect kick ok");
                        // #endregion
                    }
                    streams.push(Some(Stream::TcpDial(s)));
                    let sid = StreamId::to_wire(streams.len() - 1);
                    reply_u32_status_maybe_nonce(reply, OP_CONNECT, STATUS_OK, sid, nonce);
                    let _ = nexus_abi::debug_println("netstackd: rpc connect ok");
                } else {
                    if !*dbg_connect_kick_would_block_logged {
                        *dbg_connect_kick_would_block_logged = true;
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:netstackd: connect kick would-block");
                        // #endregion
                    }
                    if !*dbg_connect_pending_set_logged {
                        *dbg_connect_pending_set_logged = true;
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:netstackd: connect pending set");
                        // #endregion
                    }
                    *pending_dial = Some((remote, s));
                    reply_status_maybe_nonce(reply, OP_CONNECT, STATUS_WOULD_BLOCK, nonce);
                }
            }
            Err(NetError::WouldBlock) => {
                if !*dbg_connect_status_would_block_logged {
                    *dbg_connect_status_would_block_logged = true;
                    // #region agent log
                    let _ = nexus_abi::debug_println("dbg:netstackd: connect status would-block");
                    // #endregion
                }
                reply_status_maybe_nonce(reply, OP_CONNECT, STATUS_WOULD_BLOCK, nonce);
            }
            Err(_) => {
                if !*dbg_connect_status_io_logged {
                    *dbg_connect_status_io_logged = true;
                    // #region agent log
                    let _ = nexus_abi::debug_println("dbg:netstackd: connect status io");
                    // #endregion
                }
                reply_status_maybe_nonce(reply, OP_CONNECT, STATUS_IO, nonce);
            }
        }
    }
    DispatchControl::Handled
}
