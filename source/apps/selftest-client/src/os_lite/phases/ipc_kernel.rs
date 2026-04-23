// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 7 of 12 — ipc_kernel (orchestration of pure-kernel IPC probes
//!   from RFC-0005: payload roundtrip, deadline timeout, kernel-loopback,
//!   cap_move reply, sender_pid, sender_service_id, IPC soak).
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

use crate::markers::emit_line;
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

    // Kernel IPC v1 deadline semantics (RFC-0005): a past deadline should time out immediately.
    if probes::ipc_kernel::ipc_deadline_timeout_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_IPC_DEADLINE_TIMEOUT_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_IPC_DEADLINE_TIMEOUT_FAIL);
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

    // TASK-0031 VMO share floor: producer transfer -> consumer RO map -> sha256 verify.
    let vmo = probes::ipc_kernel::vmo_share_probe();
    if vmo.producer_sent {
        emit_line(crate::markers::M_VMO_PRODUCER_SENT_HANDLE);
    }
    if vmo.consumer_mapped {
        emit_line(crate::markers::M_VMO_CONSUMER_MAPPED_OK);
    }
    if vmo.sha_ok {
        emit_line(crate::markers::M_VMO_SHA256_OK);
    }
    if vmo.producer_sent && vmo.consumer_mapped && vmo.sha_ok {
        emit_line(crate::markers::M_SELFTEST_VMO_SHARE_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_VMO_SHARE_FAIL);
    }

    // IPC production-grade smoke: deterministic soak of mixed operations.
    // Keep this strictly bounded and allocation-light (avoid kernel heap exhaustion).
    if probes::ipc_kernel::ipc_soak_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_IPC_SOAK_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_IPC_SOAK_FAIL);
    }

    Ok(())
}
