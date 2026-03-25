// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OP_LOCAL_ADDR handler for netstackd IPC facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_abi::yield_;

use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::ipc::parse::parse_nonce;
use crate::os::ipc::reply::{
    append_nonce, fill_header_prefix, reply_status_maybe_nonce, status_frame,
};
use crate::os::ipc::wire::{OP_LOCAL_ADDR, STATUS_IO, STATUS_MALFORMED, STATUS_OK};

pub(crate) fn handle<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let net = &mut *ctx.net;

    static LOCAL_ADDR_LOGGED: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);
    if !LOCAL_ADDR_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed) {
        let _ = nexus_abi::debug_println("netstackd: rpc local_addr");
    }
    if req.len() != 4 && req.len() != 12 {
        reply(&status_frame(OP_LOCAL_ADDR, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let nonce = parse_nonce(req, 4);
    let Some(cfg) = net.get_ipv4_config().or_else(|| net.get_dhcp_config()) else {
        reply_status_maybe_nonce(reply, OP_LOCAL_ADDR, STATUS_IO, nonce);
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    };
    if let Some(nonce) = nonce {
        let mut rsp = [0u8; 18];
        fill_header_prefix(&mut rsp, OP_LOCAL_ADDR, STATUS_OK);
        rsp[5..9].copy_from_slice(&cfg.ip);
        rsp[9] = cfg.prefix_len;
        append_nonce(&mut rsp[10..18], nonce);
        reply(&rsp);
    } else {
        let mut rsp = [0u8; 10];
        fill_header_prefix(&mut rsp, OP_LOCAL_ADDR, STATUS_OK);
        rsp[5..9].copy_from_slice(&cfg.ip);
        rsp[9] = cfg.prefix_len;
        reply(&rsp);
    }
    DispatchControl::Handled
}
