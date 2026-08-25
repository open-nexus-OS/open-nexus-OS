// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stage / log-probe helpers for the `updated` submodule —
//!     * `updated_stage`     -- send `OP_STAGE` with the bring-up test bundle.
//!     * `updated_log_probe` -- send the unsupported-op probe (0x7f) used by
//!       the routing phase to confirm `updated` is wired and replying.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — routing + ota phases.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use nexus_ipc::KernelClient;

use crate::markers::emit_line;

use super::reply_pump::{updated_expect_status, updated_send_with_reply};
use super::types::{SYSTEM_TEST_NXS, SYSTEM_TEST_UNTRUSTED_NXS};

pub(crate) fn updated_stage(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let rsp = stage_payload(client, reply_send_slot, reply_recv_slot, pending, SYSTEM_TEST_NXS)?;
    updated_expect_status(&rsp, nexus_abi::updated::OP_STAGE)?;
    Ok(())
}

/// TASK-0198 Phase 1 deny lane: stages a validly self-signed archive whose
/// publisher is NOT in the device anchor and requires the FAILED status —
/// an OK here means the trust anchor is not enforced (the pre-fix hole).
pub(crate) fn updated_stage_untrusted_deny(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let rsp = stage_payload(
        client,
        reply_send_slot,
        reply_recv_slot,
        pending,
        SYSTEM_TEST_UNTRUSTED_NXS,
    )?;
    // Response framing: [M0, M1, VER, op|0x80, status, len:u16le, ...]
    if rsp.len() < 7 || rsp[4] != nexus_abi::updated::STATUS_FAILED {
        return Err(());
    }
    Ok(())
}

fn stage_payload(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
    payload: &[u8],
) -> core::result::Result<Vec<u8>, ()> {
    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.resize(8 + payload.len(), 0u8);
    let n = nexus_abi::updated::encode_stage_req(payload, &mut frame).ok_or(())?;
    emit_line(crate::markers::M_SELFTEST_UPDATED_STAGE_SEND);
    updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        nexus_abi::updated::OP_STAGE,
        &frame[..n],
        pending,
    )
}

pub(crate) fn updated_log_probe(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let mut frame = [0u8; 4];
    frame[0] = nexus_abi::updated::MAGIC0;
    frame[1] = nexus_abi::updated::MAGIC1;
    frame[2] = nexus_abi::updated::VERSION;
    frame[3] = 0x7f;
    let rsp =
        updated_send_with_reply(client, reply_send_slot, reply_recv_slot, 0x7f, &frame, pending)?;
    updated_expect_status(&rsp, 0x7f)?;
    Ok(())
}
