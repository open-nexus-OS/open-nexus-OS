// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Netstackd IPC facade OP_* dispatch to handler modules
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_net_os::SmoltcpVirtioNetStack;

use crate::os::facade::handlers;
use crate::os::facade::state::FacadeState;
use crate::os::ipc::handles::ReplyCapSlot;
use crate::os::ipc::reply::status_frame;
use crate::os::ipc::wire::{
    OP_ACCEPT, OP_CLOSE, OP_CONNECT, OP_ICMP_PING, OP_LISTEN, OP_LOCAL_ADDR, OP_READ, OP_UDP_BIND,
    OP_UDP_RECV_FROM, OP_UDP_SEND_TO, OP_WAIT_WRITABLE, OP_WRITE, STATUS_MALFORMED,
};

/// Control-flow result for [`dispatch_op`]: whether the facade IPC loop should `continue`
/// immediately (skipping the trailing yield) or fall through.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[must_use]
pub(crate) enum DispatchControl {
    /// Outer IPC loop should `continue` (skip trailing yield), matching prior `true` returns.
    ContinueLoop,
    /// Fall through to trailing `yield_()` in the facade loop (prior `false` returns).
    Handled,
}

/// Per-iteration inputs bundled for handler extraction (Phase-1 de-monolith).
///
/// Ownership Model:
/// - `FacadeContext` is created once per IPC turn in `runtime`.
/// - It carries exclusive borrows into the single-thread-owned runtime state.
/// - Handlers must not retain references beyond the call and may only mutate through this context.
pub(crate) struct FacadeContext<'a> {
    pub net: &'a mut SmoltcpVirtioNetStack,
    pub state: &'a mut FacadeState,
    pub now_ms: u64,
    pub bind_ip: [u8; 4],
    pub reply_slot: Option<ReplyCapSlot>,
}

/// Dispatch one decoded request. Returns [`DispatchControl::ContinueLoop`] when the outer IPC loop
/// should `continue` (skipping the trailing yield), matching the prior control flow in `runtime.rs`.
pub(crate) fn dispatch_op<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let op = req[3];
    match op {
        OP_LISTEN => handlers::listen::handle(ctx, req, reply),
        OP_ACCEPT => handlers::accept::handle(ctx, req, reply),
        OP_CONNECT => handlers::connect::handle(ctx, req, reply),
        OP_UDP_BIND => handlers::udp::handle_bind(ctx, req, reply),
        OP_UDP_SEND_TO => handlers::udp::handle_send_to(ctx, req, reply),
        OP_UDP_RECV_FROM => handlers::udp::handle_recv_from(ctx, req, reply),
        OP_WRITE => handlers::write::handle(ctx, req, reply),
        OP_READ => handlers::read::handle(ctx, req, reply),
        OP_WAIT_WRITABLE => handlers::wait_writable::handle(ctx, req, reply),
        OP_CLOSE => handlers::close::handle(ctx, req, reply),
        OP_ICMP_PING => handlers::ping::handle(ctx, req, reply),
        OP_LOCAL_ADDR => handlers::local_addr::handle(ctx, req, reply),
        _ => {
            reply(&status_frame(op, STATUS_MALFORMED));
            DispatchControl::Handled
        }
    }
}
