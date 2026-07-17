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
    // Phase C (SMP track): same-AS compute thread E2E — spawn a thread into
    // OUR address space, let it write a sentinel, reap it via wait(). The
    // thread has an empty cap table by construction (compute-only contract).
    thread_spawn_proof();

    // Phase C3 (SMP track): THE process workpool — deterministic parallel
    // compute with fence coordination; result must equal the sequential
    // reference (workers=1 ≡ workers=N contract, TASK-0276).
    workpool_proof();
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


// ——— Phase C: thread spawn proof ———

static THREAD_SENTINEL: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static mut THREAD_STACK: [u8; 16 * 1024] = [0; 16 * 1024];

extern "C" fn thread_entry(arg: usize) {
    THREAD_SENTINEL.store(arg, core::sync::atomic::Ordering::Release);
}

fn thread_spawn_proof() {
    const SENTINEL: usize = 0x7ead;
    THREAD_SENTINEL.store(0, core::sync::atomic::Ordering::Release);
    // SAFETY: the stack buffer is used exclusively by the one thread spawned
    // here; the proof is strictly sequential (spawn → join → done).
    let stack = unsafe { &mut *core::ptr::addr_of_mut!(THREAD_STACK) };
    let pid = match nexus_abi::thread::spawn_thread(thread_entry, SENTINEL, stack) {
        Ok(pid) => pid,
        Err(_) => {
            emit_line("SELFTEST: thread spawn FAIL (spawn)");
            return;
        }
    };
    let mut seen = false;
    for _ in 0..4096 {
        if THREAD_SENTINEL.load(core::sync::atomic::Ordering::Acquire) == SENTINEL {
            seen = true;
            break;
        }
        let _ = yield_();
    }
    // Reap the exited thread (trampoline exits 0 after entry returns).
    let mut reaped = false;
    for _ in 0..4096 {
        match nexus_abi::wait(pid as i32) {
            Ok((_, status)) => {
                reaped = status == 0;
                break;
            }
            Err(_) => {
                let _ = yield_();
            }
        }
    }
    if seen && reaped {
        emit_line("SELFTEST: thread spawn ok");
    } else {
        emit_line("SELFTEST: thread spawn FAIL");
    }
}


// ——— Phase C3: workpool proofs ———

const WP_TOTAL: usize = 1024;
static WP_OUT: [core::sync::atomic::AtomicU32; WP_TOTAL] =
    [const { core::sync::atomic::AtomicU32::new(0) }; WP_TOTAL];

fn wp_transform(i: usize) -> u32 {
    (i as u32).wrapping_mul(0x9E37_79B9).rotate_left(7) ^ 0x5A5A
}

extern "C" fn wp_job(start: usize, end: usize, _ctx: *mut u8) {
    for i in start..end {
        WP_OUT[i].store(wp_transform(i), core::sync::atomic::Ordering::Relaxed);
    }
}

fn workpool_proof() {
    use core::sync::atomic::Ordering;

    // Bounded contract first: run before init and invalid init are REJECTED.
    let pre_run =
        nexus_workpool::run(1, wp_job, core::ptr::null_mut(), 1_000_000_000).is_err();
    let zero_init = nexus_workpool::init(0).is_err();
    if let Err(err) = nexus_workpool::init(2) {
        emit_line(match err {
            nexus_workpool::PoolError::AbiFence => "SELFTEST: workpool bounded FAIL (fence)",
            nexus_workpool::PoolError::AbiSpawn => "SELFTEST: workpool bounded FAIL (spawn)",
            nexus_workpool::PoolError::AbiTransfer => {
                "SELFTEST: workpool bounded FAIL (transfer)"
            }
            nexus_workpool::PoolError::AbiResume => "SELFTEST: workpool bounded FAIL (resume)",
            _ => "SELFTEST: workpool bounded FAIL (init)",
        });
        return;
    }
    let double_init = nexus_workpool::init(2).is_err();
    if pre_run && zero_init && double_init {
        emit_line("SELFTEST: workpool bounded ok");
    } else {
        emit_line("SELFTEST: workpool bounded FAIL");
    }

    // Fence sanity probe: the done fence must NOT satisfy an unsignalled
    // target (catches slot/id mix-ups before the real run).
    if !nexus_workpool::pool::selftest_probe_done_fence() {
        emit_line("SELFTEST: workpool determinism FAIL (done fence satisfied unsignalled)");
        return;
    }

    // Determinism: parallel result must equal the sequential reference.
    for slot in WP_OUT.iter() {
        slot.store(0, Ordering::Relaxed);
    }
    match nexus_workpool::run(WP_TOTAL, wp_job, core::ptr::null_mut(), 5_000_000_000) {
        Ok(()) => {
            let mut bad_lo = 0usize;
            let mut bad_hi = 0usize;
            for i in 0..WP_TOTAL {
                if WP_OUT[i].load(Ordering::Acquire) != wp_transform(i) {
                    if i < WP_TOTAL / 2 {
                        bad_lo += 1;
                    } else {
                        bad_hi += 1;
                    }
                }
            }
            if bad_lo == 0 && bad_hi == 0 {
                emit_line("SELFTEST: workpool determinism ok");
                // C4: worker 0 pins itself to CPU 0 (present on every SMP
                // config), so at least one round-tripped pin is REQUIRED.
                if nexus_workpool::pool::selftest_pinned() >= 1 {
                    emit_line("SELFTEST: workpool affinity ok");
                } else {
                    emit_line("SELFTEST: workpool affinity FAIL (no pin)");
                }
            } else if bad_lo > 0 && bad_hi == 0 {
                emit_line("SELFTEST: workpool determinism FAIL (lo chunk)");
            } else if bad_lo == 0 && bad_hi > 0 {
                emit_line("SELFTEST: workpool determinism FAIL (hi chunk)");
            } else {
                let (alive, woke, done) = nexus_workpool::pool::selftest_debug();
                emit_line(match (alive, woke, done) {
                    (0, _, _) => "SELFTEST: workpool determinism FAIL (both, alive=0)",
                    (1, _, _) => "SELFTEST: workpool determinism FAIL (both, alive=1)",
                    (_, _, 0) => "SELFTEST: workpool determinism FAIL (both, done=0)",
                    (_, _, 1) => "SELFTEST: workpool determinism FAIL (both, done=1)",
                    _ => "SELFTEST: workpool determinism FAIL (both, alive=2 done=2)",
                });
            }
        }
        Err(_) => {
            let (alive, woke, done) = nexus_workpool::pool::selftest_debug();
            let self_sig = nexus_workpool::pool::selftest_probe_job_selfsignal();
            emit_line(match (alive, woke, done, self_sig) {
                (_, 0, _, true) => {
                    "SELFTEST: workpool determinism FAIL (run, woke=0 selfsig=ok)"
                }
                (_, 0, _, false) => {
                    "SELFTEST: workpool determinism FAIL (run, woke=0 selfsig=FAIL)"
                }
                (_, _, 0, _) => "SELFTEST: workpool determinism FAIL (run, woke>0 done=0)",
                _ => "SELFTEST: workpool determinism FAIL (run, woke>0 done>0)",
            });
        }
    }
}
