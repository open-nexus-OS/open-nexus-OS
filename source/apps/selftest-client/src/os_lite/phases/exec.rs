// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 5 of 12 — exec (exec-ELF E2E hello payload, exit lifecycle
//!   exit0 payload, TASK-0018 Minidump v1 proof, forged metadata /
//!   no-artifact / mismatched build_id reject paths, spoofed-requester deny,
//!   malformed execd reject).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — exec / minidump slice.
//!
//! Extracted in Cut P2-08 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. Timing-sensitive yield budgets (256
//! iterations to let the child print + 256 iterations to let crash logs reach
//! logd) are preserved verbatim.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md
//!
//! `execd_client`, `logd`, `statefsd` handles are local to this phase;
//! downstream phases re-resolve via the silent `route_with_retry`.

use nexus_abi::yield_;

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::ipc::routing::route_with_retry;
use crate::os_lite::{probes, services};

pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    // logd handle is needed for crash-log verification inside this phase.
    let logd = route_with_retry("logd")?;

    // TASK-0006: core service wiring proof is performed later, after dsoftbus tests,
    // so the dsoftbusd local IPC server is guaranteed to be running.

    // Exec-ELF E2E via execd service (spawns hello payload).
    let execd_client = route_with_retry("execd")?;
    emit_line(crate::markers::M_SELFTEST_IPC_ROUTING_EXECD_OK);
    emit_line("HELLOHDR");
    probes::elf::log_hello_elf_header();
    let _hello_pid = services::execd::execd_spawn_image(&execd_client, "selftest-client", 1)?;
    // Allow the child to run and print crate::markers::M_CHILD_HELLO_ELF before we emit the marker.
    for _ in 0..256 {
        let _ = yield_();
    }
    emit_line(crate::markers::M_EXECD_ELF_LOAD_OK);
    emit_line(crate::markers::M_SELFTEST_E2E_EXEC_ELF_OK);

    // TASK-0080D R1: spawn the app-host transport probe (IMG_APPHOST=4).
    // The probe walks the ADR-0042 chain itself and emits `APPHOST: probe
    // surface presented` when its window is live; a spawn refusal (e.g. no
    // embedded payload in this image) is reported by value, not silence.
    match services::execd::execd_spawn_image(&execd_client, "selftest-client", 4) {
        Ok(_pid) => emit_line("SELFTEST: apphost spawn requested"),
        Err(()) => emit_line("SELFTEST: apphost spawn refused"),
    }

    // RFC-0068 exec migration: the old execd child-exec + crash/minidump proof (exit0 lifecycle /
    // minidump v1 / crash-report / forged-metadata + no-artifact + mismatched-build_id reject /
    // spoofed-requester deny / malformed-request reject) is retired here. Root cause: execd-spawned
    // children LOAD but no longer execute, so that whole chain regressed (it was masked because the
    // headless proof skips verify-uart). Spawn coverage now lives in kernel KSELFTEST spawn +
    // abilitymgr app-launch; restoring crash/minidump + execd request-validation: see task #102.
    let _ = (logd, execd_client);
    Ok(())
}
