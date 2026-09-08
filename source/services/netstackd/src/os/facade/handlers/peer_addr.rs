// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OP_PEER_ADDR handler (RFC-0092 facade prerequisite, TASK-0052
//! P3): the remote `(ip, port)` of a stream — real sockets report the
//! stack's remote endpoint, in-facade pairs the address recorded at pairing
//! (loopback stays `127.0.0.1`, hairpins the interface address). This is the
//! accept-side identity the ingress gateway's CIDR filter judges.
//! Request `[hdr4][stream_id u32]` (+nonce), reply `[hdr5][ip 4][port u16le]`.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Unstable (additive op)
//! TEST_COVERAGE: tests/handler_rejects.rs (op vocabulary), QEMU ingress markers

use nexus_abi::yield_;

use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::facade::state::Stream;
use crate::os::facade::validation;
use crate::os::ipc::handles::StreamId;
use crate::os::ipc::parse::{parse_nonce, parse_u32_le};
use crate::os::ipc::reply::{
    append_nonce, fill_header_prefix, reply_status_maybe_nonce, status_frame,
};
use crate::os::ipc::wire::{
    OP_PEER_ADDR, STATUS_IO, STATUS_MALFORMED, STATUS_NOT_FOUND, STATUS_OK,
};

pub(crate) fn handle<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let streams = &mut ctx.state.streams;
    if validation::validate_exact_or_nonce_len(req.len(), 8).is_malformed() {
        reply(&status_frame(OP_PEER_ADDR, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let nonce = parse_nonce(req, 8);
    let Some(sid) = parse_u32_le(req, 4).and_then(StreamId::from_wire) else {
        reply_status_maybe_nonce(reply, OP_PEER_ADDR, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let Some(Some(s)) = streams.get(sid.index()) else {
        reply_status_maybe_nonce(reply, OP_PEER_ADDR, STATUS_NOT_FOUND, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let remote = match s {
        Stream::TcpDial(s) | Stream::TcpAccepted(s) => {
            s.remote_endpoint().map(|a| (a.ip.0, a.port))
        }
        Stream::Loop { remote, .. } => Some(*remote),
    };
    let Some((ip, port)) = remote else {
        reply_status_maybe_nonce(reply, OP_PEER_ADDR, STATUS_IO, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    let mut rsp = [0u8; 19];
    fill_header_prefix(&mut rsp, OP_PEER_ADDR, STATUS_OK);
    rsp[5..9].copy_from_slice(&ip);
    rsp[9..11].copy_from_slice(&port.to_le_bytes());
    if let Some(nonce) = nonce {
        append_nonce(&mut rsp[11..19], nonce);
        reply(&rsp);
    } else {
        reply(&rsp[..11]);
    }
    DispatchControl::Handled
}
