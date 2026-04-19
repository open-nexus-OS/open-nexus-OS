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

    // Exit lifecycle: spawn exit0 payload, wait for termination, and print markers.
    let exit_pid = services::execd::execd_spawn_image(&execd_client, "selftest-client", 2)?;
    // Wait for exit; child prints crate::markers::M_CHILD_EXIT0_START itself.
    let status = services::execd::wait_for_pid(&execd_client, exit_pid).unwrap_or(-1);
    services::execd::emit_line_with_pid_status(exit_pid, status);
    emit_line(crate::markers::M_SELFTEST_CHILD_EXIT_OK);

    // TASK-0018: Minidump v1 proof. Spawn a deterministic non-zero exit (42), then
    // verify execd appended crash metadata and wrote a bounded minidump path.
    let statefsd = route_with_retry("statefsd").ok();
    let crash_pid = services::execd::execd_spawn_image(&execd_client, "selftest-client", 3)?;
    if let Some(statefsd) = statefsd.as_ref() {
        if services::statefs::grant_statefs_caps_to_child(statefsd, crash_pid).is_err() {
            emit_line(crate::markers::M_SELFTEST_MINIDUMP_CAP_GRANT_FAIL);
        }
    }
    let crash_status = services::execd::wait_for_pid(&execd_client, crash_pid).unwrap_or(-1);
    services::execd::emit_line_with_pid_status(crash_pid, crash_status);
    let mut dump_written = false;
    if let Some(statefsd) = statefsd.as_ref() {
        if let Ok((build_id, dump_path, dump_bytes)) = services::statefs::locate_minidump_for_crash(
            statefsd,
            crash_pid,
            crash_status,
            "demo.minidump",
        ) {
            if services::execd::execd_report_exit_with_dump(
                &execd_client,
                crash_pid,
                crash_status,
                build_id.as_str(),
                dump_path.as_str(),
                dump_bytes.as_slice(),
            )
            .is_ok()
            {
                dump_written = true;
            } else {
                emit_line(crate::markers::M_SELFTEST_MINIDUMP_REPORT_FAIL);
            }
        } else {
            emit_line(crate::markers::M_SELFTEST_MINIDUMP_LOCATE_FAIL);
        }
    } else {
        emit_line(crate::markers::M_SELFTEST_MINIDUMP_ROUTE_FAIL);
    }
    // Give cooperative scheduling a deterministic window to deliver the crash append to logd.
    for _ in 0..256 {
        let _ = yield_();
    }
    let saw_crash =
        services::logd::logd_query_contains_since_paged(&logd, 0, b"crash").unwrap_or(false);
    let saw_name = services::logd::logd_query_contains_since_paged(&logd, 0, b"demo.minidump")
        .unwrap_or(false);
    let saw_event = services::logd::logd_query_contains_since_paged(&logd, 0, b"event=crash.v1")
        .unwrap_or(false);
    let saw_build_id =
        services::logd::logd_query_contains_since_paged(&logd, 0, b"build_id=").unwrap_or(false);
    let saw_dump_path =
        services::logd::logd_query_contains_since_paged(&logd, 0, b"dump_path=/state/crash/")
            .unwrap_or(false);
    let crash_logged = saw_crash && saw_name && saw_event && saw_build_id && saw_dump_path;
    if crash_status == 42 && crash_logged {
        emit_line(crate::markers::M_SELFTEST_CRASH_REPORT_OK);
    } else {
        if !saw_crash {
            emit_line(crate::markers::M_SELFTEST_CRASH_REPORT_MISSING_CRASH);
        }
        if !saw_name {
            emit_line(crate::markers::M_SELFTEST_CRASH_REPORT_MISSING_DEMO_MINIDUMP);
        }
        if !saw_event {
            emit_line(crate::markers::M_SELFTEST_CRASH_REPORT_MISSING_EVENT_CRASH_V1);
        }
        if !saw_build_id {
            emit_line(crate::markers::M_SELFTEST_CRASH_REPORT_MISSING_BUILD_ID);
        }
        if !saw_dump_path {
            emit_line(crate::markers::M_SELFTEST_CRASH_REPORT_MISSING_DUMP_PATH_STATE_CRASH);
        }
        emit_line(crate::markers::M_SELFTEST_CRASH_REPORT_FAIL);
    }
    let dump_present = route_with_retry("statefsd")
        .ok()
        .and_then(|statefsd| services::statefs::statefs_has_crash_dump(&statefsd).ok())
        .unwrap_or(false);
    if crash_status == 42 && dump_written && crash_logged && dump_present {
        emit_line(crate::markers::M_SELFTEST_MINIDUMP_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_MINIDUMP_FAIL);
    }

    // Negative Soll-Verhalten: forged metadata publish must be rejected fail-closed.
    let forged_status = services::execd::execd_report_exit_with_dump_status(
        &execd_client,
        crash_pid,
        crash_status,
        "binvalid",
        "/state/crash/forged.demo.minidump.nmd",
        b"forged",
    )
    .unwrap_or(0xff);
    if forged_status != 0 {
        emit_line(crate::markers::M_SELFTEST_MINIDUMP_FORGED_METADATA_REJECTED);
    } else {
        emit_line(crate::markers::M_SELFTEST_MINIDUMP_FORGED_METADATA_FAIL);
    }
    let no_artifact_status = services::execd::execd_report_exit_with_dump_status_legacy(
        &execd_client,
        crash_pid,
        crash_status,
        "binvalid",
        "/state/crash/forged.demo.minidump.nmd",
    )
    .unwrap_or(0xff);
    if no_artifact_status != 0 {
        emit_line(crate::markers::M_SELFTEST_MINIDUMP_NO_ARTIFACT_METADATA_REJECTED);
    } else {
        emit_line(crate::markers::M_SELFTEST_MINIDUMP_NO_ARTIFACT_METADATA_FAIL);
    }
    let mismatch_status = if let Some(statefsd) = statefsd.as_ref() {
        if let Ok((_, _, dump_bytes)) = services::statefs::locate_minidump_for_crash(
            statefsd,
            crash_pid,
            crash_status,
            "demo.minidump",
        ) {
            services::execd::execd_report_exit_with_dump_status(
                &execd_client,
                crash_pid,
                crash_status,
                "binvalid",
                "/state/crash/child.demo.minidump.nmd",
                dump_bytes.as_slice(),
            )
            .unwrap_or(0xff)
        } else {
            0xff
        }
    } else {
        0xff
    };
    if mismatch_status != 0 {
        emit_line(crate::markers::M_SELFTEST_MINIDUMP_MISMATCHED_BUILD_ID_REJECTED);
    } else {
        emit_line(crate::markers::M_SELFTEST_MINIDUMP_MISMATCHED_BUILD_ID_FAIL);
    }

    // Security: spoofed requester must be denied because execd binds identity to sender_service_id.
    let rsp = services::execd::execd_spawn_image_raw_requester(&execd_client, "demo.testsvc", 1)?;
    if rsp.len() == 9
        && rsp[0] == b'E'
        && rsp[1] == b'X'
        && rsp[2] == 1
        && rsp[3] == (1 | 0x80)
        && rsp[4] == 4
    {
        emit_line(crate::markers::M_SELFTEST_EXEC_DENIED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_EXEC_DENIED_FAIL);
    }

    // Malformed execd request should return a structured error response.
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(
        &clock,
        &execd_client,
        b"bad",
        core::time::Duration::from_millis(200),
    )
    .map_err(|_| ())?;
    let rsp = nexus_ipc::budget::recv_budgeted(
        &clock,
        &execd_client,
        core::time::Duration::from_millis(200),
    )
    .map_err(|_| ())?;
    if rsp.len() == 9 && rsp[0] == b'E' && rsp[1] == b'X' && rsp[2] == 1 && rsp[4] != 0 {
        emit_line(crate::markers::M_SELFTEST_EXECD_MALFORMED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_EXECD_MALFORMED_FAIL);
    }

    let _ = (logd, execd_client, statefsd);
    Ok(())
}
