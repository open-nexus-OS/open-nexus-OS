// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OP_ICMP_PING handler for netstackd IPC facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_abi::yield_;

use crate::os::facade::dispatch::{DispatchControl, FacadeContext};
use crate::os::facade::ping;
use crate::os::facade::validation;
use crate::os::ipc::parse::{parse_ipv4_at, parse_nonce, parse_u16_le};
use crate::os::ipc::reply::{
    reply_status_maybe_nonce, reply_u16_field_status_maybe_nonce, status_frame,
};
use crate::os::ipc::wire::{OP_ICMP_PING, STATUS_MALFORMED, STATUS_OK, STATUS_TIMED_OUT};

pub(crate) fn handle<R: FnMut(&[u8])>(
    ctx: &mut FacadeContext<'_>,
    req: &[u8],
    reply: &mut R,
) -> DispatchControl {
    let net = &mut *ctx.net;

    if validation::validate_exact_or_nonce_len(req.len(), 10).is_malformed() {
        reply(&status_frame(OP_ICMP_PING, STATUS_MALFORMED));
        let _ = yield_();
        return DispatchControl::ContinueLoop;
    }
    let nonce = parse_nonce(req, 10);
    let target_ip = parse_ipv4_at(req, 4).unwrap_or([0u8; 4]);
    let timeout_ms = parse_u16_le(req, 8).unwrap_or(0) as u64;
    let ping_start = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;

    match net.icmp_ping(target_ip, ping_start, timeout_ms) {
        Ok(rtt_ms) => {
            let rtt_capped = ping::cap_rtt_ms(rtt_ms);
            reply_u16_field_status_maybe_nonce(reply, OP_ICMP_PING, STATUS_OK, rtt_capped, nonce);
        }
        Err(_) => {
            reply_status_maybe_nonce(reply, OP_ICMP_PING, STATUS_TIMED_OUT, nonce);
        }
    }
    DispatchControl::Handled
}
