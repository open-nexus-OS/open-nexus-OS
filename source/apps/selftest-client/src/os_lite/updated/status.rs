// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Status / boot-attempt helpers for the `updated` submodule —
//!     * `updated_get_status`   -- decode (active, pending, tries_left, healthy).
//!     * `updated_boot_attempt` -- consume one boot attempt and return the slot
//!       that ran (used to drive A/B health flow).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — ota phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use nexus_ipc::KernelClient;

use super::reply_pump::{updated_expect_status, updated_send_with_reply};
use super::types::SlotId;

pub(crate) fn updated_get_status(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(SlotId, Option<SlotId>, u8, bool), ()> {
    let mut frame = [0u8; 4];
    let n = nexus_abi::updated::encode_get_status_req(&mut frame).ok_or(())?;
    let rsp = updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        nexus_abi::updated::OP_GET_STATUS,
        &frame[..n],
        pending,
    )?;
    let payload = updated_expect_status(&rsp, nexus_abi::updated::OP_GET_STATUS)?;
    if payload.len() != 4 {
        return Err(());
    }
    let active = match payload[0] {
        1 => SlotId::A,
        2 => SlotId::B,
        _ => return Err(()),
    };
    let pending_slot = match payload[1] {
        0 => None,
        1 => Some(SlotId::A),
        2 => Some(SlotId::B),
        _ => None,
    };
    Ok((active, pending_slot, payload[2], payload[3] != 0))
}

pub(crate) fn updated_boot_attempt(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<Option<SlotId>, ()> {
    let mut frame = [0u8; 4];
    let n = nexus_abi::updated::encode_boot_attempt_req(&mut frame).ok_or(())?;
    let rsp = updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        nexus_abi::updated::OP_BOOT_ATTEMPT,
        &frame[..n],
        pending,
    )?;
    let payload = updated_expect_status(&rsp, nexus_abi::updated::OP_BOOT_ATTEMPT)?;
    if payload.len() != 1 {
        return Ok(None);
    }
    Ok(match payload[0] {
        1 => Some(SlotId::A),
        2 => Some(SlotId::B),
        _ => None,
    })
}
