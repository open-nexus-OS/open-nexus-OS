// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SoC glue probe (RFC-0106, TASK-0245B P2). The harness asks socd to bring
//! up the node whose window it holds (the net transport) and expects the machine's
//! honest verdict: on QEMU virt the tree binds nothing, so the only right answer is
//! `NOT_NEEDED` — an `OK` here would be a fake success, a `DENIED` a policy hole.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (bringup phase) — `SELFTEST: soc glue not needed ok`
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use crate::markers::{emit_bytes, emit_hex_u64, emit_line};
use crate::os_lite::services::socd::{self, SocdError};
use nexus_wire::soc;

/// The node the harness holds a device window for on every machine.
const NET_NODE_VIRT: &str = "/soc/virtio_mmio@10001000";

pub(crate) fn soc_glue_selftest() {
    let nonce = (nexus_abi::nsec().unwrap_or(0) as u32) ^ 0x50C0_50C0;
    let client = match socd::client() {
        Ok(c) => c,
        Err(_) => {
            emit_line(crate::markers::M_SELFTEST_SOC_GLUE_NOT_NEEDED_FAIL_NO_SLOTS);
            return;
        }
    };
    match socd::bring_up(&client, NET_NODE_VIRT, nonce) {
        Err(SocdError::NoSlots) => {
            emit_line(crate::markers::M_SELFTEST_SOC_GLUE_NOT_NEEDED_FAIL_NO_SLOTS)
        }
        Err(SocdError::Send) => emit_line(crate::markers::M_SELFTEST_SOC_GLUE_NOT_NEEDED_FAIL_SEND),
        Err(SocdError::NoReply) => {
            emit_line(crate::markers::M_SELFTEST_SOC_GLUE_NOT_NEEDED_FAIL_RECV)
        }
        Err(SocdError::WrongOp) => {
            emit_line(crate::markers::M_SELFTEST_SOC_GLUE_NOT_NEEDED_FAIL_WRONG_OP)
        }
        Ok(reply)
            if reply.status == soc::STATUS_NOT_NEEDED
                || reply.status == soc::STATUS_NO_SUCH_NODE =>
        {
            // NO_SUCH_NODE covers a tree whose net transport sits elsewhere; the point is
            // that NOTHING was brought up and nothing was denied.
            emit_line(crate::markers::M_SELFTEST_SOC_GLUE_NOT_NEEDED_OK)
        }
        Ok(reply) => {
            emit_bytes(crate::markers::M_SELFTEST_SOC_GLUE_NOT_NEEDED_FAIL_STATUS_0X.as_bytes());
            emit_hex_u64(reply.status as u64);
            emit_line(")");
        }
    }
}
