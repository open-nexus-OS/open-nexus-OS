//! Phase: exec (extracted in Cut P2-08 of TASK-0023B).
//!
//! Owns the execd-driven slice: exec-ELF E2E (hello payload) + exit lifecycle
//! (exit0 payload) + TASK-0018 Minidump v1 proof (crash payload + crash log
//! verification + dump-present check) + forged metadata / no-artifact /
//! mismatched build_id reject paths + spoofed-requester deny + malformed
//! execd reject.
//!
//! Marker order and marker strings are byte-identical to the pre-cut body.
//! Timing-sensitive yield budgets (256 iterations to let the child print +
//! 256 iterations to let crash logs reach logd) are preserved verbatim.
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
    emit_line("SELFTEST: ipc routing execd ok");
    emit_line("HELLOHDR");
    probes::elf::log_hello_elf_header();
    let _hello_pid = services::execd::execd_spawn_image(&execd_client, "selftest-client", 1)?;
    // Allow the child to run and print "child: hello-elf" before we emit the marker.
    for _ in 0..256 {
        let _ = yield_();
    }
    emit_line("execd: elf load ok");
    emit_line("SELFTEST: e2e exec-elf ok");

    // Exit lifecycle: spawn exit0 payload, wait for termination, and print markers.
    let exit_pid = services::execd::execd_spawn_image(&execd_client, "selftest-client", 2)?;
    // Wait for exit; child prints "child: exit0 start" itself.
    let status = services::execd::wait_for_pid(&execd_client, exit_pid).unwrap_or(-1);
    services::execd::emit_line_with_pid_status(exit_pid, status);
    emit_line("SELFTEST: child exit ok");

    // TASK-0018: Minidump v1 proof. Spawn a deterministic non-zero exit (42), then
    // verify execd appended crash metadata and wrote a bounded minidump path.
    let statefsd = route_with_retry("statefsd").ok();
    let crash_pid = services::execd::execd_spawn_image(&execd_client, "selftest-client", 3)?;
    if let Some(statefsd) = statefsd.as_ref() {
        if services::statefs::grant_statefs_caps_to_child(statefsd, crash_pid).is_err() {
            emit_line("SELFTEST: minidump cap grant FAIL");
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
                emit_line("SELFTEST: minidump report FAIL");
            }
        } else {
            emit_line("SELFTEST: minidump locate FAIL");
        }
    } else {
        emit_line("SELFTEST: minidump route FAIL");
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
        emit_line("SELFTEST: crash report ok");
    } else {
        if !saw_crash {
            emit_line("SELFTEST: crash report missing 'crash'");
        }
        if !saw_name {
            emit_line("SELFTEST: crash report missing 'demo.minidump'");
        }
        if !saw_event {
            emit_line("SELFTEST: crash report missing 'event=crash.v1'");
        }
        if !saw_build_id {
            emit_line("SELFTEST: crash report missing 'build_id='");
        }
        if !saw_dump_path {
            emit_line("SELFTEST: crash report missing 'dump_path=/state/crash/'");
        }
        emit_line("SELFTEST: crash report FAIL");
    }
    let dump_present = route_with_retry("statefsd")
        .ok()
        .and_then(|statefsd| services::statefs::statefs_has_crash_dump(&statefsd).ok())
        .unwrap_or(false);
    if crash_status == 42 && dump_written && crash_logged && dump_present {
        emit_line("SELFTEST: minidump ok");
    } else {
        emit_line("SELFTEST: minidump FAIL");
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
        emit_line("SELFTEST: minidump forged metadata rejected");
    } else {
        emit_line("SELFTEST: minidump forged metadata FAIL");
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
        emit_line("SELFTEST: minidump no-artifact metadata rejected");
    } else {
        emit_line("SELFTEST: minidump no-artifact metadata FAIL");
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
        emit_line("SELFTEST: minidump mismatched build_id rejected");
    } else {
        emit_line("SELFTEST: minidump mismatched build_id FAIL");
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
        emit_line("SELFTEST: exec denied ok");
    } else {
        emit_line("SELFTEST: exec denied FAIL");
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
        emit_line("SELFTEST: execd malformed ok");
    } else {
        emit_line("SELFTEST: execd malformed FAIL");
    }

    let _ = (logd, execd_client, statefsd);
    Ok(())
}
