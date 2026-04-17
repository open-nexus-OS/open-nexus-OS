//! TASK-0023B P2-14: stage / log-probe helpers for the updated submodule.
//!
//! Hosts:
//!   * `updated_stage`     -- send `OP_STAGE` with the bring-up test bundle.
//!   * `updated_log_probe` -- send the unsupported-op probe (0x7f) used by the
//!     routing phase to confirm `updated` is wired and replying.
//!
//! Behavior is byte-for-byte identical to the pre-split implementation.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use nexus_ipc::KernelClient;

use crate::markers::emit_line;

use super::reply_pump::{updated_expect_status, updated_send_with_reply};
use super::types::SYSTEM_TEST_NXS;

pub(crate) fn updated_stage(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let mut frame = Vec::with_capacity(8 + SYSTEM_TEST_NXS.len());
    frame.resize(8 + SYSTEM_TEST_NXS.len(), 0u8);
    let n = nexus_abi::updated::encode_stage_req(SYSTEM_TEST_NXS, &mut frame).ok_or(())?;
    emit_line("SELFTEST: updated stage send");
    let rsp = updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        nexus_abi::updated::OP_STAGE,
        &frame[..n],
        pending,
    )?;
    updated_expect_status(&rsp, nexus_abi::updated::OP_STAGE)?;
    Ok(())
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
    let rsp = updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        0x7f,
        &frame,
        pending,
    )?;
    updated_expect_status(&rsp, 0x7f)?;
    Ok(())
}
