// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host walltime client (RFC-0076): one bounded
//! `OP_GET_WALLTIME` round-trip to timed over the `svc.time` SDK route.
//! Split out of `effect_host.rs` (structure gate).

use crate::effect_host::{call_reply, AppEffectHost};

/// RFC-0076: one bounded `OP_GET_WALLTIME` round-trip to timed over the
/// `svc.time` route (slot presence-probed like every SDK route). `None` =
/// route ungranted / timed unreachable / walltime unanchored — the clock
/// stays at its placeholder, never fakes time.
pub(crate) fn walltime_now() -> Option<u64> {
    let send_slot = AppEffectHost::svc_send_slot("time")?;
    let mut req = [0u8; 8];
    req[0] = b'T';
    req[1] = b'M';
    req[2] = 1;
    req[3] = 4; // OP_GET_WALLTIME
    req[4..8].copy_from_slice(&0x4157_0001u32.to_le_bytes());
    let mut resp = [0u8; 64];
    let len = call_reply(send_slot, &req, &mut resp)?;
    let rsp = &resp[..len];
    if rsp.len() != 17
        || rsp[0] != b'T'
        || rsp[1] != b'M'
        || rsp[2] != 1
        || rsp[3] != (4 | 0x80)
        || rsp[4] != 0
    {
        return None;
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&rsp[9..17]);
    Some(u64::from_le_bytes(b))
}
