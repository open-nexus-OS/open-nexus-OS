// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 7 of 12 — ipc_kernel (orchestration of pure-kernel IPC probes
//!   from RFC-0005: payload roundtrip, kernel-loopback,
//!   cap_move reply, sender_pid, sender_service_id, IPC soak, IPC bench).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — kernel IPC slice.
//!
//! Extracted in Cut P2-03 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. This phase performs no service routing;
//! it only invokes pure-kernel probes exposed via `probes::ipc_kernel::*`.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use crate::markers::{emit_bytes, emit_line, emit_u64};
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::probes;

pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    // Kernel IPC v1 payload copy roundtrip (RFC-0005):
    // send payload via `SYSCALL_IPC_SEND_V1`, then recv it back via `SYSCALL_IPC_RECV_V1`.
    if probes::ipc_kernel::ipc_payload_roundtrip().is_ok() {
        emit_line(crate::markers::M_SELFTEST_IPC_PAYLOAD_ROUNDTRIP_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_IPC_PAYLOAD_ROUNDTRIP_FAIL);
    }

    // Exercise `nexus-ipc` kernel backend (NOT service routing) deterministically:
    // send to bootstrap endpoint and receive our own message back.
    if probes::ipc_kernel::nexus_ipc_kernel_loopback_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_NEXUS_IPC_KERNEL_LOOPBACK_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_NEXUS_IPC_KERNEL_LOOPBACK_FAIL);
    }

    // IPC v1 capability move (CAP_MOVE): request/reply without pre-shared reply endpoints.
    if probes::ipc_kernel::cap_move_reply_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_IPC_CAP_MOVE_REPLY_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_IPC_CAP_MOVE_REPLY_FAIL);
    }

    // IPC sender attribution: kernel writes sender pid into MsgHeader.dst on receive.
    if probes::ipc_kernel::sender_pid_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_IPC_SENDER_PID_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_IPC_SENDER_PID_FAIL);
    }

    // IPC sender identity binding: kernel returns sender service_id via ipc_recv_v2 metadata.
    if probes::ipc_kernel::sender_service_id_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_IPC_SENDER_SERVICE_ID_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_IPC_SENDER_SERVICE_ID_FAIL);
    }

    // RFC-0068 exec migration retired the TASK-0031 VMO-share floor (the producer blocked on a
    // consumer child that no longer ran). Its unscheduled probe and hand-built consumer ELF — which
    // shared a fixed child slot by convention — were deleted in TASK-0324 P4f-6; a restored proof
    // belongs on the app-child path with a declared child slot.

    // TASK-0054C P4a: request + reply in ONE trap, reply home in registers.
    if probes::ipc_kernel::ipc_call_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_IPC_CALL_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_IPC_CALL_FAIL);
    }

    // TASK-0054C P4b: reply + next request in ONE trap.
    if probes::ipc_kernel::reply_recv_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_IPC_REPLY_RECV_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_IPC_REPLY_RECV_FAIL);
    }

    // TASK-0054C P3b: the hard payload cap answers with E2BIG, not EINVAL.
    if probes::ipc_kernel::oversize_reject_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_IPC_OVERSIZE_REJECTED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_IPC_OVERSIZE_REJECTED_FAIL);
    }

    // IPC production-grade smoke: deterministic soak of mixed operations.
    // Keep this strictly bounded and allocation-light (avoid kernel heap exhaustion).
    if probes::ipc_kernel::ipc_soak_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_IPC_SOAK_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_IPC_SOAK_FAIL);
    }

    // TASK-0054C P1 (RFC-0096 Phase 1): the request/reply round trip as a number.
    // Printed, never asserted — the budgets are calibrated from this line.
    match probes::ipc_kernel::ipc_bench_probe() {
        Ok(probes::ipc_kernel::BenchResult { rt_us, rounds }) => {
            emit_bytes(crate::markers::M_SELFTEST_IPC_BENCH_RT.as_bytes());
            emit_u64(rt_us);
            emit_bytes(b"us n=");
            emit_u64(u64::from(rounds));
            emit_line(")");
        }
        Err(()) => emit_line(crate::markers::M_SELFTEST_IPC_BENCH_FAIL),
    }

    Ok(())
}
