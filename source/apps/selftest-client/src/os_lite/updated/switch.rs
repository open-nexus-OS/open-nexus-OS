//! TASK-0023B P2-14: switch helper for the updated submodule.
//!
//! Hosts `updated_switch` -- send `OP_SWITCH` with the requested boot-attempt
//! budget. Behavior is byte-for-byte identical to the pre-split implementation.

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
