// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Switch helper for the `updated` submodule — `updated_switch`
//!   sends `OP_SWITCH` with the requested boot-attempt budget.
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

pub(crate) fn updated_switch(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    tries_left: u8,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let mut frame = [0u8; 5];
    let n = nexus_abi::updated::encode_switch_req(tries_left, &mut frame).ok_or(())?;
    let rsp = updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        nexus_abi::updated::OP_SWITCH,
        &frame[..n],
        pending,
    )?;
    updated_expect_status(&rsp, nexus_abi::updated::OP_SWITCH)?;
    Ok(())
}
