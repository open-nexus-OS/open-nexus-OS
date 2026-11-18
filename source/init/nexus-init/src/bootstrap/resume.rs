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
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Resume every spawned service EXCEPT the display + input device drivers
/// (`gpud`/`windowd`/`inputd`/`hidrawd`) so policyd can service MMIO policy checks
/// during the grant phase. Drivers stay suspended (zero CPU) until their MMIO is
/// granted + routes wired — a driver resumed before its MMIO busy-yields waiting
/// for it, wasting scheduler cycles that slow the very grant phase it is blocked on
/// (`hidrawd` previously raced here — see its `entry_to_ready_ms`). IPC wiring
/// happens after grants.

/// P1: apply the declarative CPU placement (service_topology::placement_for)
/// right before waking the service. The role's CPU set is derived from the CPUs the kernel
/// reports online (RFC-0118), so it is a set the kernel admits on every machine: a refusal
/// is a failure, never a reason to try a wider set.
static AFFINITY_APPLIED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static AFFINITY_FAILED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// The online CPUs as the kernel reported them, asked once (0: not asked yet — a machine
/// always has one online).
static ONLINE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

fn online_cpus() -> Option<nexus_abi::sched::CpuSet> {
    use core::sync::atomic::Ordering;
    let known = nexus_abi::sched::CpuSet::from_bits(ONLINE.load(Ordering::Relaxed));
    if !known.is_empty() {
        return Some(known);
    }
    let asked = nexus_abi::sched::online().ok()?;
    ONLINE.store(asked.bits(), Ordering::Relaxed);
    Some(asked)
}

pub(crate) fn apply_affinity(chan_name: &str, pid: u32) {
    let set =
        online_cpus().map(|online| crate::service_topology::placement_for(chan_name).set(online));
    let applied = set.is_some_and(|set| nexus_abi::sched::set_affinity_for(pid, set).is_ok());
    if applied {
        AFFINITY_APPLIED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    } else {
        // RFC-0068: per-item lines only for the failure path.
        AFFINITY_FAILED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        debug_write_str("init: affinity FAIL svc=");
        debug_write_str(chan_name);
        debug_write_str(" set=0x");
        debug_write_hex(set.map_or(0, |set| set.bits() as usize));
        debug_write_byte(b'\n');
    }
}

/// ADR-0096: the soft-realtime chain's scheduler class, `Interactive`, set right before the
/// service's wake — init is the one raiser (a service may only lower itself). A `Normal` service
/// is left as spawned. Counted like the placement; a refusal is a per-item line.
static QOS_RAISED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static QOS_FAILED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

pub(crate) fn apply_qos(chan_name: &str, pid: u32) {
    if !crate::service_topology::soft_realtime(chan_name) {
        return;
    }
    if nexus_abi::task_qos_set_for(pid, nexus_abi::QosClass::Interactive).is_ok() {
        QOS_RAISED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    } else {
        QOS_FAILED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        debug_write_str("init: qos FAIL svc=");
        debug_write_str(chan_name);
        debug_write_byte(b'\n');
    }
}

/// RFC-0120: who holds which time. init claims the machine's root time right once and divides
/// it (TASK-0365 P1):
/// - the display chain's right, and from it one right per chain service that holds time
///   (`service_topology::CHAIN`) — handed to the service as a capability (`TIME_RIGHT_SLOT`)
///   and bound to it as a reservation right before its wake, so a service's first run is
///   already its reservation's. A respawned service is handed the same right again (its old
///   reservation ended with it);
/// - the apps' right (`apps_time_ppm`, in grains of `APPS_GRAIN_PPM`), handed to the app
///   spawner, which hands each app a part of it.
///
/// init keeps the capability of every right it handed down: a revoke takes it back in one
/// step. A refusal is a per-item line and the service runs fair at its class's weight. Slots
/// are stored plus one (0: not held).
static ROOT_RIGHT: AtomicU32 = AtomicU32::new(0);
static CHAIN_RIGHT: AtomicU32 = AtomicU32::new(0);
static APPS_RIGHT: AtomicU32 = AtomicU32::new(0);
static SERVICE_RIGHTS: [AtomicU32; crate::service_topology::CHAIN.len()] =
    [const { AtomicU32::new(0) }; crate::service_topology::CHAIN.len()];
static TIME_HANDED: AtomicUsize = AtomicUsize::new(0);
static TIME_BOUND: AtomicUsize = AtomicUsize::new(0);
static TIME_FAILED: AtomicUsize = AtomicUsize::new(0);

fn held_right(cell: &AtomicU32) -> Option<nexus_abi::Cap> {
    let held = cell.load(Ordering::Relaxed);
    (held != 0).then(|| held - 1)
}

/// The machine's root right: claimed at the first ask.
fn root_right() -> Option<nexus_abi::Cap> {
    held_right(&ROOT_RIGHT).or_else(|| {
        let root = nexus_abi::time_right::claim_root().ok()?;
        ROOT_RIGHT.store(root + 1, Ordering::Relaxed);
        Some(root)
    })
}

/// The right `cell` keeps: derived from `parent` at the first ask (`share_ppm`, in grains of
/// `grain_ppm` — 0: the parent's).
fn derived(
    cell: &AtomicU32,
    parent: Option<nexus_abi::Cap>,
    share_ppm: u32,
    grain_ppm: u32,
) -> Option<nexus_abi::Cap> {
    held_right(cell).or_else(|| {
        let right = nexus_abi::time_right::derive(parent?, share_ppm, grain_ppm).ok()?;
        cell.store(right + 1, Ordering::Relaxed);
        Some(right)
    })
}

/// Hands `right` down to the child `pid`, into the fleet's slot.
fn hand_down(right: nexus_abi::Cap, pid: u32) -> bool {
    let slot = crate::service_topology::TIME_RIGHT_SLOT;
    nexus_abi::time_right::hand_down(right, pid, slot).is_ok()
}

fn time_right_failed(chan_name: &str, share_ppm: u32, handed: bool) {
    TIME_FAILED.fetch_add(1, Ordering::Relaxed);
    debug_write_str("init: time right FAIL svc=");
    debug_write_str(chan_name);
    debug_write_str(" share_ppm=0x");
    debug_write_hex(share_ppm as usize);
    debug_write_str(if handed { " handed=1\n" } else { " handed=0\n" });
}

pub(crate) fn apply_time_right(chan_name: &str, pid: u32) {
    if chan_name == crate::service_topology::APP_SPAWNER {
        return hand_apps_right(chan_name, pid);
    }
    let Some((row, r)) = crate::service_topology::reservation_for(chan_name) else {
        return;
    };
    let chain = derived(&CHAIN_RIGHT, root_right(), crate::service_topology::display_time_ppm(), 0);
    let right = derived(&SERVICE_RIGHTS[row], chain, r.share_ppm, 0);
    let handed = right.is_some_and(|right| hand_down(right, pid));
    let bound = handed
        && right.is_some_and(|right| {
            nexus_abi::time_right::bind_for(right, pid, r.share_ppm, r.period_ns).is_ok()
        });
    if handed {
        TIME_HANDED.fetch_add(1, Ordering::Relaxed);
    }
    if bound {
        TIME_BOUND.fetch_add(1, Ordering::Relaxed);
    } else {
        time_right_failed(chan_name, r.share_ppm, handed);
    }
}

/// The apps' right into the app spawner. A spawner that is spawned again starts over: the old
/// right is revoked — every app's right below it ends in that one step — and a new one derived.
/// A machine too small for one grain has no apps' right: its apps run in the fair class.
fn hand_apps_right(chan_name: &str, pid: u32) {
    if let Some(old) = held_right(&APPS_RIGHT) {
        let _ = nexus_abi::time_right::revoke(old);
        let _ = nexus_abi::cap_close(old);
        APPS_RIGHT.store(0, Ordering::Relaxed);
    }
    let root = root_right();
    let total = root.and_then(|root| nexus_abi::time_right::held(root).ok()).map_or(0, |h| h.0);
    let share = crate::service_topology::apps_time_ppm(total);
    if share == 0 {
        return;
    }
    let right = derived(&APPS_RIGHT, root, share, crate::service_topology::APPS_GRAIN_PPM);
    if right.is_some_and(|right| hand_down(right, pid)) {
        TIME_HANDED.fetch_add(1, Ordering::Relaxed);
    } else {
        time_right_failed(chan_name, share, false);
    }
}

/// The time rights as the kernel holds them now, in ppm of one CPU: what the root was given
/// and what init delegated from it (the chain's right and the apps'), what of the chain
/// services' rights is bound, and how many rights were handed down and bound.
fn time_right_summary() {
    let held = |cell: &AtomicU32| {
        held_right(cell).and_then(|slot| nexus_abi::time_right::held(slot).ok()).unwrap_or((0, 0))
    };
    let (root, root_own) = held(&ROOT_RIGHT);
    let (apps, _) = held(&APPS_RIGHT);
    let bound: u32 =
        SERVICE_RIGHTS.iter().map(|cell| held(cell)).map(|(total, own)| total - own).sum();
    debug_write_str("init: time rights (root_ppm=0x");
    debug_write_hex(root as usize);
    debug_write_str(" delegated_ppm=0x");
    debug_write_hex(root.saturating_sub(root_own) as usize);
    debug_write_str(" apps_ppm=0x");
    debug_write_hex(apps as usize);
    debug_write_str(" bound_ppm=0x");
    debug_write_hex(bound as usize);
    debug_write_str(" handed=0x");
    debug_write_hex(TIME_HANDED.load(Ordering::Relaxed));
    debug_write_str(" bound=0x");
    debug_write_hex(TIME_BOUND.load(Ordering::Relaxed));
    debug_write_str(" fail=0x");
    debug_write_hex(TIME_FAILED.load(Ordering::Relaxed));
    debug_write_str(")\n");
}

/// One RFC-0068-style summary line for the whole placement pass, one for the classes and one
/// for the time rights.
pub(crate) fn affinity_summary() {
    time_right_summary();
    debug_write_str("init: affinity applied n=0x");
    debug_write_hex(AFFINITY_APPLIED.load(core::sync::atomic::Ordering::Relaxed));
    debug_write_str(" fail=0x");
    debug_write_hex(AFFINITY_FAILED.load(core::sync::atomic::Ordering::Relaxed));
    debug_write_str(" online=0x");
    debug_write_hex(ONLINE.load(core::sync::atomic::Ordering::Relaxed) as usize);
    debug_write_byte(b'\n');
    debug_write_str("init: qos interactive n=0x");
    debug_write_hex(QOS_RAISED.load(core::sync::atomic::Ordering::Relaxed));
    debug_write_str(" fail=0x");
    debug_write_hex(QOS_FAILED.load(core::sync::atomic::Ordering::Relaxed));
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

/// Materializes the resume set (TASK-0050 PR-5): announces the target truth, resumes wave 2 —
/// the target's graph on this machine — and resumes the display/input drivers only when the
/// graph includes them (recovery keeps them suspended).
pub(crate) fn materialize_graph(
    ctrls: &[CtrlChannel],
    set: crate::boot_graph::ResumeSet,
    init_fold: bool,
    init_misc: &mut nexus_event::SpanTally,
) {
    let graph = set.graph();
    crate::bootstrap::diag::emit_marker_atomic(
        &[b"init: stage graph target=", graph.label().as_bytes()],
        None,
    );
    resume_wave2(ctrls, set);
    if crate::boot_graph::includes(graph, "windowd") {
        resume_drivers(ctrls, init_fold, init_misc);
    } else {
        crate::bootstrap::diag::emit_marker_atomic(
            &[b"init: stage graph drivers skipped (", graph.label().as_bytes(), b")"],
            None,
        );
    }
}

/// Wave 2: resume the remaining non-drivers of the resume set. A service the machine cannot
/// carry stays suspended and is named with the plane it stands on — the device tree decided,
/// so the service never finds out by trying.
pub(crate) fn resume_wave2(ctrls: &[CtrlChannel], set: crate::boot_graph::ResumeSet) {
    for chan in ctrls {
        if let Some(plane) = set.withheld_for(chan.svc_name) {
            crate::bootstrap::diag::emit_marker_atomic(
                &[
                    b"init: withheld svc=",
                    chan.svc_name.as_bytes(),
                    b" (no ",
                    plane.name().as_bytes(),
                    b" plane)",
                ],
                None,
            );
        }
    }
    resume_non_drivers_where(ctrls, |name| !crate::boot_graph::in_core(name) && set.runs(name));
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
        apply_qos(chan.svc_name, chan.pid);
        apply_time_right(chan.svc_name, chan.pid);
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
        apply_qos(chan.svc_name, chan.pid);
        apply_time_right(chan.svc_name, chan.pid);
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
