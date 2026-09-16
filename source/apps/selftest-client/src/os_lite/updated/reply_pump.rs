// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The `updated` exchange for the OTA probes:
//!     * `updated_send_with_reply` -- one waited request/response on the dedicated pair.
//!     * `updated_expect_status`   -- response framing + status validation.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — ota phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use nexus_ipc::KernelClient;

use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_line};

pub(crate) fn updated_expect_status<'a>(
    rsp: &'a [u8],
    op: u8,
) -> core::result::Result<&'a [u8], ()> {
    if rsp.len() < 7 {
        emit_line(crate::markers::M_SELFTEST_UPDATED_RSP_SHORT);
        return Err(());
    }
    if rsp[0] != nexus_abi::updated::MAGIC0
        || rsp[1] != nexus_abi::updated::MAGIC1
        || rsp[2] != nexus_abi::updated::VERSION
    {
        emit_bytes(crate::markers::M_SELFTEST_UPDATED_RSP_MAGIC.as_bytes());
        emit_hex_u64(rsp[0] as u64);
        emit_byte(b' ');
        emit_hex_u64(rsp[1] as u64);
        emit_byte(b' ');
        emit_hex_u64(rsp[2] as u64);
        emit_byte(b'\n');
        return Err(());
    }
    if rsp[3] != (op | 0x80) || rsp[4] != nexus_abi::updated::STATUS_OK {
        emit_bytes(crate::markers::M_SELFTEST_UPDATED_RSP_STATUS.as_bytes());
        emit_hex_u64(rsp[3] as u64);
        emit_byte(b' ');
        emit_hex_u64(rsp[4] as u64);
        emit_byte(b'\n');
        return Err(());
    }
    let len = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
    if rsp.len() != 7 + len {
        emit_line(crate::markers::M_SELFTEST_UPDATED_RSP_LEN_MISMATCH);
        return Err(());
    }
    Ok(&rsp[7..])
}

/// ONE exchange with `updated` over the harness' DEDICATED updated pair (a `SharedResponse`
/// route: no cap moves, updated answers on its own endpoint). TASK-0054C P2-e deleted the three
/// things that sat around it: a 256-frame non-blocking pre-drain of the harness' SHARED `@reply`
/// inbox that CONSUMED and discarded other probes' awaited replies, a second pre-drain of this
/// pair, and a `VecDeque` stash for out-of-order answers. Nothing is out of order any more —
/// probes run sequentially and every exchange waits for its own answer — so a reply for another
/// op is a late answer to an ask that was abandoned, which no longer happens; it is reported
/// once and dropped.
pub(crate) fn updated_send_with_reply(
    client: &KernelClient,
    op: u8,
    frame: &[u8],
) -> core::result::Result<alloc::vec::Vec<u8>, ()> {
    let (updated_send, updated_recv) = client.slots();
    if nexus_ipc::exchange::send_request(updated_send, frame).is_err() {
        emit_line(crate::markers::M_SELFTEST_UPDATED_SEND_FAIL);
        return Err(());
    }
    let mut buf = [0u8; 512];
    let mut logged_noise = false;
    loop {
        let Ok(n) = nexus_ipc::exchange::recv_response(updated_recv, &mut buf) else {
            emit_line(crate::markers::M_SELFTEST_UPDATED_RECV_TIMEOUT);
            return Err(());
        };
        let n = n.min(buf.len());
        if n < 4
            || buf[0] != nexus_abi::updated::MAGIC0
            || buf[1] != nexus_abi::updated::MAGIC1
            || buf[2] != nexus_abi::updated::VERSION
            || (buf[3] & 0x80) == 0
        {
            continue;
        }
        if buf[3] == (op | 0x80) {
            return Ok(buf[..n].to_vec());
        }
        if !logged_noise {
            logged_noise = true;
            emit_bytes(crate::markers::M_SELFTEST_UPDATED_RSP_OTHER_OP_0X.as_bytes());
            emit_hex_u64(buf[3] as u64);
            if n >= 5 {
                emit_bytes(b" st=0x");
                emit_hex_u64(buf[4] as u64);
            }
            emit_byte(b'\n');
        }
    }
}
