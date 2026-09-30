// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SoC glue probes (RFC-0106, TASK-0245B P2/P3). On every machine the harness asks
//! socd to bring up the node whose window it holds (the net transport) and expects the
//! machine's honest verdict: on QEMU virt the tree binds nothing, so the only right answer is
//! `NOT_NEEDED` — an `OK` here would be a fake success, a `DENIED` a policy hole. On the board
//! it also asks for every display node the tree names (TASK-0245B P3): `OK` only when socd saw
//! power domain 7 report on, the reset released and `hmclk` on at its demanded rate — socd's
//! own line carries every register it touched, before and after.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (bringup phase) — `SELFTEST: soc glue not needed ok`;
//!   the board ladder (`scripts/board-test.sh`) — `SELFTEST: soc glue display ok`
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use crate::markers::{emit_bytes, emit_hex_u64, emit_line};
use crate::os_lite::boot_cfg::{self, Machine};
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

/// The display pipeline's compatibles — the board tree's data: the controller and the encoder.
const DISPLAY_COMPATIBLES: [&str; 2] = ["spacemit,dpu-online2", "spacemit,hdmi"];

/// On the board: socd brings up every display node the tree names. `OK` for each, or the first
/// verdict that is not — a machine whose tree names none has no display to prove, and says so.
pub(crate) fn soc_glue_display_selftest() {
    if boot_cfg::machine() != Machine::Board {
        return;
    }
    let Some(fdt) = boot_cfg::tree() else {
        emit_line(crate::markers::M_SELFTEST_SOC_GLUE_DISPLAY_FAIL_NO_DISPLAY_NODE);
        return;
    };
    let Ok(client) = socd::client() else {
        emit_line(crate::markers::M_SELFTEST_SOC_GLUE_DISPLAY_FAIL_IPC);
        return;
    };
    let nonce = (nexus_abi::nsec().unwrap_or(0) as u32) ^ 0xD15B_0A7D;
    let mut nodes = 0usize;
    for node in fdt.find_compatible(&DISPLAY_COMPATIBLES) {
        let mut buf = [0u8; 128];
        let Some(path) = node.path_into(&mut buf) else {
            emit_line(crate::markers::M_SELFTEST_SOC_GLUE_DISPLAY_FAIL_NO_DISPLAY_NODE);
            return;
        };
        match socd::bring_up(&client, path, nonce.wrapping_add(nodes as u32)) {
            Ok(reply) if reply.status == soc::STATUS_OK => nodes += 1,
            Ok(reply) => {
                emit_bytes(crate::markers::M_SELFTEST_SOC_GLUE_DISPLAY_FAIL_STATUS_0X.as_bytes());
                emit_hex_u64(reply.status as u64);
                emit_line(")");
                return;
            }
            Err(_) => {
                emit_line(crate::markers::M_SELFTEST_SOC_GLUE_DISPLAY_FAIL_IPC);
                return;
            }
        }
    }
    if nodes == 0 {
        emit_line(crate::markers::M_SELFTEST_SOC_GLUE_DISPLAY_FAIL_NO_DISPLAY_NODE);
    } else {
        emit_line(crate::markers::M_SELFTEST_SOC_GLUE_DISPLAY_OK);
    }
}
