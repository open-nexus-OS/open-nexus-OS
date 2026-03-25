// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OP_WAIT_WRITABLE handler for netstackd IPC facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_abi::yield_;

use crate::os::config::TCP_READY_SPIN_BUDGET;
use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::facade::state::Stream;
use crate::os::facade::validation;
use crate::os::ipc::handles::StreamId;
use crate::os::ipc::parse::{parse_nonce, parse_u32_le};
use crate::os::ipc::reply::{reply_status_maybe_nonce, status_frame};
use crate::os::ipc::wire::{
    OP_WAIT_WRITABLE, STATUS_MALFORMED, STATUS_NOT_FOUND, STATUS_OK, STATUS_WOULD_BLOCK,
};

pub(crate) fn handle<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let streams = &mut ctx.state.streams;

    if validation::validate_exact_or_nonce_len(req.len(), 8).is_malformed() {
        reply(&status_frame(OP_WAIT_WRITABLE, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let nonce = parse_nonce(req, 8);
    let Some(sid) = parse_u32_le(req, 4) else {
        reply(&crate::os::ipc::reply::status_frame(OP_WAIT_WRITABLE, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let Some(sid) = StreamId::from_wire(sid) else {
        reply_status_maybe_nonce(reply, OP_WAIT_WRITABLE, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let sid0 = sid.index();
    let status = match streams.get_mut(sid0) {
        Some(Some(Stream::TcpDial(s) | Stream::TcpAccepted(s))) => {
            if s.wait_writable_bounded(TCP_READY_SPIN_BUDGET) {
                STATUS_OK
            } else {
                STATUS_WOULD_BLOCK
            }
        }
        Some(Some(Stream::Loop { .. })) => STATUS_OK,
        _ => STATUS_NOT_FOUND,
    };
    reply_status_maybe_nonce(reply, OP_WAIT_WRITABLE, status, nonce);
    DispatchControl::Handled
}
