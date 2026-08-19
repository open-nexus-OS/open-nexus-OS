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
    // P0 two-window: bring-up is complete — log the burst maxima and reset
    // so the boot-end `bkl budget` gate judges STEADY STATE (this ladder).
    nexus_abi::sched::bkl_budget_reset();

    // Task #14 (SMP track): register-soak — seed every callee/caller-saved
    // GPR with a pattern, spin across several timer preemptions, verify. A
    // corrupted register here is the self-localizing proof for the
    // "f32/alloc-heavy compute is non-deterministic in this process" bug.
    probes::soaks::regsoak_proof();

    // TASK-0288: interactive floor — this task runs Interactive QoS; across
    // a yield storm under live system background load, no single scheduling
    // gap may exceed the floor budget (result-proof; generous bound so MTTCG
    // SMP=2 timing cannot flake it).
    probes::soaks::ui_runtime_floor_proof();

    // B6 (TASK-0042): the sched ABI applied from userspace — affinity and
    // shares round-trip through the REAL syscalls on this task.
    probes::soaks::sched_applied_proof();

    // Task #14: the direct symptom — building and rasterizing the SAME SVG
    // plan multiple times in THIS process must be deterministic and match
    // the host-pinned golden digest. This was provably broken during the D4
    // work (empty/varying plans); keep it as a standing detector.
    probes::soaks::svg_local_determinism_proof();

    // Task #14 companions: isolate the ingredient — pure f32 compute (no
    // alloc) and pure alloc traffic (no f32) with known answers.
    probes::soaks::f32_soak_proof();
    probes::soaks::alloc_soak_proof();
    probes::soaks::memset_soak_proof();

    // Phase C (SMP track): same-AS compute thread E2E — spawn a thread into
    // OUR address space, let it write a sentinel, reap it via wait(). The
    // thread has an empty cap table by construction (compute-only contract).
    probes::soaks::thread_spawn_proof();

    // Phase C3 (SMP track): THE process workpool — deterministic parallel
    // compute with fence coordination; result must equal the sequential
    // reference (workers=1 ≡ workers=N contract, TASK-0276).
    probes::soaks::workpool_proof();
    crate::os_lite::probes::pinched::pinched_selftest();

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
        Ok(_pid) => emit_line(crate::markers::M_SELFTEST_APPHOST_SPAWN_REQUESTED),
        Err(()) => emit_line(crate::markers::M_SELFTEST_APPHOST_SPAWN_REFUSED),
    }

    // TASK-0049 reanimation (2026-08-19): the chain retired under "RFC-0068
    // exec migration" is restored verbatim from af0c7a8d^. Root cause of the
    // regression ("children LOAD but no longer execute") was execd never
    // resuming its suspended-spawned children — fixed in 0080D R1 (execd
    // os_lite.rs "#102 ROOT CAUSE FIX"); boot-verified 2026-08-19 via
    // `child: hello-elf` appearing before the parent-side markers.

    // Exit lifecycle: spawn exit0 payload, wait for termination, and print markers.
    let exit_pid = services::execd::execd_spawn_image(&execd_client, "selftest-client", 2)?;
    // Wait for exit; child prints crate::markers::M_CHILD_EXIT0_START itself.
    // ADR-0056: the reply also carries the kernel-attributed exit reason.
    let (status, exit0_tag) =
        match services::execd::wait_for_pid_with_reason(&execd_client, exit_pid) {
            Some((code, tag, _cause)) => (code, Some(tag)),
            None => (-1, None),
        };
    services::execd::emit_line_with_pid_status(exit_pid, status);
    emit_line(crate::markers::M_SELFTEST_CHILD_EXIT_OK);

    // ADR-0056 exit-reason truth (TASK-0049): a clean exit must report
    // reason=clean (tag 0), and a child that dereferences VA 0 must report
    // reason=fault (tag 2) — kernel truth, not exit-code guesswork. The
    // demo.fault payload prints `child: fault start`, then loads from 0; its
    // unreachable fallback exits 7, which fails this assert loudly.
    let fault_pid = services::execd::execd_spawn_image(&execd_client, "selftest-client", 5)?;
    let fault_tag = services::execd::wait_for_pid_with_reason(&execd_client, fault_pid)
        .map(|(_code, tag, _cause)| tag);
    if status == 0 && exit0_tag == Some(0) && fault_tag == Some(2) {
        emit_line(crate::markers::M_SELFTEST_EXIT_REASON_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_EXIT_REASON_FAIL);
    }

    // TASK-0018: Minidump v1 proof. Spawn a deterministic non-zero exit (42), then
    // verify execd appended crash metadata and wrote a bounded minidump path.
    // The statefs route lands in the child's slots 7/8 via EXECD before resume
    // (grants-before-resume, TASK-0049 reanimation) — the old selftest-side
    // post-spawn transfer raced the child's exit once children actually run.
    let statefsd = route_with_retry("statefsd").ok();
    let crash_pid = services::execd::execd_spawn_image(&execd_client, "selftest-client", 3)?;
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
