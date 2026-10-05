// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service resume phases — init-lite resumes non-driver services before
//! the MMIO-grant phase, then the display/input device drivers last (after their
//! MMIO is granted + routes wired). Extracted from `orchestrator::run_bootstrap`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable

use crate::bootstrap::diag::il;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::*;

/// Resume every spawned service EXCEPT the display + input device drivers
/// (`gpud`/`windowd`/`inputd`/`hidrawd`) so policyd can service MMIO policy checks
/// during the grant phase. Drivers stay suspended (zero CPU) until their MMIO is
/// granted + routes wired — a driver resumed before its MMIO busy-yields waiting
/// for it, wasting scheduler cycles that slow the very grant phase it is blocked on
/// (`hidrawd` previously raced here — see its `entry_to_ready_ms`). IPC wiring
/// happens after grants.

/// P1: apply the declarative CPU placement (service_topology::affinity_for)
/// right before waking the service. Best-effort: a rejected mask (e.g. all
/// target cpus offline) leaves the inherited mask; the kernel clamps.
static AFFINITY_APPLIED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static AFFINITY_CLAMPED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static AFFINITY_FAILED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// "Any CPU" — the placement a mask degrades to when none of its target
/// CPUs is online (a 1-CPU boot has no cpu1-3 to put background work on).
const AFFINITY_ANY: usize = 0b1111;

fn apply_affinity(chan_name: &str, pid: u32) {
    let mask = crate::service_topology::affinity_for(chan_name);
    if nexus_abi::sched::set_affinity_for(pid, mask as usize).is_ok() {
        AFFINITY_APPLIED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    } else if nexus_abi::sched::set_affinity_for(pid, AFFINITY_ANY).is_ok() {
        // The kernel refuses a mask that misses every online CPU (fewer
        // CPUs than the placement assumes): degrade to any-CPU, counted in
        // the summary line rather than a per-item FAIL — nothing failed.
        AFFINITY_CLAMPED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    } else {
        // RFC-0068: per-item lines only for the failure path.
        AFFINITY_FAILED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        debug_write_str("init: affinity FAIL svc=");
        debug_write_str(chan_name);
        debug_write_str(" mask=0x");
        debug_write_hex(mask as usize);
        debug_write_byte(b'\n');
    }
}

/// One RFC-0068-style summary line for the whole placement pass.
pub(crate) fn affinity_summary() {
    debug_write_str("init: affinity applied n=0x");
    debug_write_hex(AFFINITY_APPLIED.load(core::sync::atomic::Ordering::Relaxed));
    debug_write_str(" clamped=0x");
    debug_write_hex(AFFINITY_CLAMPED.load(core::sync::atomic::Ordering::Relaxed));
    debug_write_str(" fail=0x");
    debug_write_hex(AFFINITY_FAILED.load(core::sync::atomic::Ordering::Relaxed));
    debug_write_byte(b'\n');
}

/// Wave 0 (TASK-0321 P4): the CORE plane the volume spawn pass needs —
/// policy authority, the SoC glue owner (TASK-0246 P4c), the block device owner and the
/// volume verifier — and NOTHING else. Every other service stays suspended until its server
/// pair is distributed: a resumed service without its pair retries its
/// route probe over init's control channel, and each retry parks a
/// CAP_MOVEd reply cap in init's 256-slot table (8 per service, the ctrl
/// queue depth) — with the whole core running through a ~100 ms pass that
/// exhausted the table (`abi:no-space`, blk-plane wiring FAIL).
const PLANE: &[&str] = &["policyd", "socd", "blkd", "bundlemgrd"];

/// The plane's policy authority — resumed first and alone: every grant the plane makes asks
/// it, and the disk's owner must find its disk in place when it starts (TASK-0246 P4b — no
/// owner waits for its grant).
const AUTHORITY: &str = "policyd";

/// The SoC glue owner (RFC-0106) — resumed second, once its windows are granted: the disk's
/// owner asks it to bring the disk's node up before it touches the controller (TASK-0246 P4c).
const SOC_GLUE: &str = "socd";

pub(crate) fn in_plane(name: &str) -> bool {
    PLANE.contains(&name)
}

/// Wave 0a: the policy authority.
pub(crate) fn resume_authority(ctrls: &[CtrlChannel]) {
    resume_non_drivers_where(ctrls, |name| name == AUTHORITY);
}

/// Wave 0b: the SoC glue owner.
pub(crate) fn resume_soc_glue(ctrls: &[CtrlChannel]) {
    resume_non_drivers_where(ctrls, |name| name == SOC_GLUE);
}

/// Wave 0c: the rest of the plane — the block owner (its disk already granted) and the
/// volume verifier.
pub(crate) fn resume_plane(ctrls: &[CtrlChannel]) {
    resume_non_drivers_where(ctrls, |name| in_plane(name) && name != AUTHORITY && name != SOC_GLUE);
}

/// Wave 1 (TASK-0050 PR-5): resume the rest of the always-on CORE graph —
/// the boot target is unknown until the boot-attempt handshake, and the
/// core is exactly what that handshake needs (plus the recovery floor).
/// Runs AFTER the bulk server-pair distribution (see `PLANE`).
pub(crate) fn resume_core(ctrls: &[CtrlChannel]) {
    resume_non_drivers_where(ctrls, |name| crate::boot_graph::in_core(name) && !in_plane(name));
}

/// Materializes the resolved boot graph (TASK-0050 PR-5): announces the
/// target truth, resumes wave 2, and resumes the display/input drivers
/// only when the graph includes them (recovery keeps them suspended).
pub(crate) fn materialize_graph(
    ctrls: &[CtrlChannel],
    graph: crate::boot_graph::BootGraph,
    init_fold: bool,
    init_misc: &mut nexus_event::SpanTally,
) {
    crate::bootstrap::diag::emit_marker_atomic(
        &[b"init: stage graph target=", graph.label().as_bytes()],
        None,
    );
    resume_wave2(ctrls, graph);
    if crate::boot_graph::includes(graph, "windowd") {
        resume_drivers(ctrls, init_fold, init_misc);
    } else {
        crate::bootstrap::diag::emit_marker_atomic(
            &[b"init: stage graph drivers skipped (", graph.label().as_bytes(), b")"],
            None,
        );
    }
}

/// Wave 2: resume the remaining non-drivers the resolved target includes.
pub(crate) fn resume_wave2(ctrls: &[CtrlChannel], graph: crate::boot_graph::BootGraph) {
    resume_non_drivers_where(ctrls, |name| {
        !crate::boot_graph::in_core(name) && crate::boot_graph::includes(graph, name)
    });
}

/// The display + input device drivers: resumed after their MMIO grants and route wiring, never
/// in a hand-picked order (ADR-0062).
pub(crate) fn is_driver(name: &str) -> bool {
    matches!(name, "gpud" | "windowd" | "inputd" | "hidrawd" | "xhcid")
}

fn resume_non_drivers_where(ctrls: &[CtrlChannel], pred: impl Fn(&str) -> bool) {
    for chan in ctrls {
        if is_driver(chan.svc_name) {
            continue;
        }
        if !pred(chan.svc_name) {
            continue;
        }
        apply_affinity(chan.svc_name, chan.pid);
        match nexus_abi::task_resume(chan.pid) {
            Ok(()) => {}
            Err(e) => {
                debug_write_bytes(b"init: resume fail pid=0x");
                debug_write_hex(chan.pid as usize);
                debug_write_str(" svc=");
                debug_write_str(chan.svc_name);
                debug_write_str(" err=0x");
                debug_write_hex(e as usize);
                debug_write_byte(b'\n');
            }
        }
    }
}

/// Resume the display + input device-driver services after their MMIO grants and route wiring.
///
/// ADR-0062: the ORDER is no longer a contract. This list used to be hand-sorted — gpud first so
/// the scanout handoff was live before windowd presented, hidrawd last so its MMIO and inputd's
/// route already existed — which made a comment the only thing standing between the boot and a
/// black screen. Dependencies are now carried by the two mechanisms that can actually express
/// them: a route ask parks until its target can answer (routing v2), and a service does not run
/// before its stage opens (the boot-stage fence). So they are resumed in whatever order the
/// channel list holds, which is exactly what proves the order does not matter.
pub(crate) fn resume_drivers(
    ctrls: &[CtrlChannel],
    init_fold: bool,
    init_misc: &mut nexus_event::SpanTally,
) {
    for chan in ctrls.iter().filter(|c| is_driver(c.svc_name)) {
        apply_affinity(chan.svc_name, chan.pid);
        match nexus_abi::task_resume(chan.pid) {
            Ok(()) => {
                if il(init_misc, init_fold, chan.svc_name) {
                    debug_write_bytes(b"init: deferred resume ");
                    debug_write_str(chan.svc_name);
                    debug_write_byte(b'\n');
                }
            }
            Err(e) => {
                debug_write_bytes(b"init: deferred resume fail svc=");
                debug_write_str(chan.svc_name);
                debug_write_bytes(b" err=0x");
                debug_write_hex(e as usize);
                debug_write_byte(b'\n');
            }
        }
    }
    affinity_summary();
}
