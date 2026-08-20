// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: init's supervision sweep (TASK-0049B PR-B1 — the OBSERVATION
//! half). Boot services are init's kernel children, and `wait` is
//! parent-bound — so init is the ONLY process that can reap them; before
//! this module a dying service was reaped by nobody and its death was
//! invisible (the RED decision in the 0049B ledger is thereby forced, not
//! chosen). Each responder round drains `wait_nohang_with_reason` and
//! announces every death ONCE with kernel truth (ADR-0056):
//! `init: service exit name=<svc> reason=<label> code=0x<hex>`.
//! PR-B2 adds the ACTION half for the standing detector: a supervised
//! `demo.fault` probe is restarted with backoff by the pure engine
//! (`supervision_engine`) until the crash-loop cap parks it — restart,
//! backoff and cap proven with REAL kernel fault exits, every boot.
//!
//! Boot proof: the one-shot `demo.exit0` probe proves the sweep
//! (`SELFTEST: init supervision sweep ok`); the fault injector proves
//! restart (`SELFTEST: supervision restart ok`) and the cap
//! (`init: crash-loop blocked svc=fault-probe …` +
//! `SELFTEST: crash-loop cap ok`). All gated.
//!
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (probe verdict, every boot) +
//!   service_topology SUPERVISION host tests (the policy SSOT this consumes).
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§2 supervision)

use crate::bootstrap::helpers::{debug_write_byte, debug_write_bytes, debug_write_hex};
use crate::bootstrap::CtrlChannel;
use crate::service_supervision::{BackoffSpec, RestartPolicy};
use crate::supervision_engine::{EngineDecision, SupervisedChild};
use nexus_abi::ExitReason;

/// The fault-injector probe's OWN backoff (test-profile, not the service
/// default): tight delays so the standing detector finishes in well under a
/// second of scheduled backoff; same threshold/window semantics as the SSOT.
const INJECTOR_BACKOFF: BackoffSpec =
    BackoffSpec { initial_ms: 10, factor: 2, max_ms: 50, window_s: 60, threshold: 5 };

/// Per-boot sweep state: the probe pid and its one-shot verdict latch.
pub(crate) struct SupervisionSweep {
    /// Pid of the one-shot exit0 probe child, until its verdict is emitted.
    probe_pid: Option<u32>,
    /// The standing restart/crash-loop detector (ADR-0048 doctrine).
    injector: FaultInjector,
}

/// Standing fault injector (TASK-0049B PR-B2): a supervised `demo.fault`
/// child crashes deterministically; the engine restarts it with backoff
/// until the crash-loop cap parks it — proving restart, backoff scheduling
/// and the cap with REAL kernel fault exits, every boot.
struct FaultInjector {
    /// Live probe pid (`None` before first spawn / after the cap fired).
    pid: Option<u32>,
    /// The pure engine driving restart decisions.
    child: SupervisedChild,
    /// Attempt number of the pending scheduled restart (marker payload).
    pending_attempt: u32,
    /// Fault exits observed (2nd exit == a restarted child ran again).
    exits_seen: u32,
    /// One-shot latch for `SELFTEST: supervision restart ok`.
    restart_ok_emitted: bool,
}

impl SupervisionSweep {
    /// Spawns the one-shot supervision probe (best-effort — a spawn failure
    /// is announced and costs the verdict marker, never the boot).
    pub(crate) fn start() -> Self {
        // Same suspended-spawn discipline as every exec child (#102): the
        // kernel starts it Suspended; resume AFTER any grants (the probe
        // needs none — it prints nothing and exits 0).
        let probe_pid = match nexus_abi::exec(demo_exit0::DEMO_EXIT0_ELF, 4, 0) {
            Ok(pid) => {
                if nexus_abi::task_resume(pid).is_err() {
                    debug_write_bytes(b"init: FAIL supervision probe resume\n");
                    None
                } else {
                    Some(pid)
                }
            }
            Err(_) => {
                debug_write_bytes(b"init: FAIL supervision probe spawn\n");
                None
            }
        };
        // Standing fault injector: same suspended-spawn discipline.
        let injector_pid = spawn_fault_probe();
        Self {
            probe_pid,
            injector: FaultInjector {
                pid: injector_pid,
                child: SupervisedChild::new(RestartPolicy::OnFailure, INJECTOR_BACKOFF),
                pending_attempt: 0,
                exits_seen: 0,
                restart_ok_emitted: false,
            },
        }
    }

    /// One bounded drain of exited children (called once per responder
    /// round). Every reaped death is announced exactly once — the kernel
    /// reap itself is the once-latch.
    pub(crate) fn sweep(
        &mut self,
        channels: &[CtrlChannel],
        route_table: &mut crate::route_table::RouteTable,
    ) {
        // init's child count is the boot service fleet; 8 per round drains
        // any realistic burst without letting a pathological loop spin.
        for _ in 0..8 {
            match nexus_abi::wait_nohang_with_reason() {
                Ok(Some((pid, code, reason))) => {
                    if self.probe_pid == Some(pid) {
                        self.probe_pid = None;
                        if reason == ExitReason::Clean && code == 0 {
                            debug_write_bytes(b"SELFTEST: init supervision sweep ok\n");
                        } else {
                            debug_write_bytes(b"SELFTEST: init supervision sweep FAIL\n");
                        }
                        continue;
                    }
                    if self.injector.pid == Some(pid) {
                        self.injector.on_exit(code, reason);
                        continue;
                    }
                    announce_service_exit(pid, code, reason, channels, route_table);
                }
                Ok(None) | Err(_) => break,
            }
        }
        self.injector.tick();
    }
}

impl FaultInjector {
    /// The injector child exited: feed the engine, emit the one-shot
    /// restart proof on the SECOND exit (a restarted child ran again), and
    /// park loudly when the cap fires.
    fn on_exit(&mut self, code: i32, reason: ExitReason) {
        self.pid = None;
        self.exits_seen += 1;
        if self.exits_seen == 2 && !self.restart_ok_emitted {
            self.restart_ok_emitted = true;
            debug_write_bytes(b"SELFTEST: supervision restart ok\n");
        }
        match self.child.on_exit(reason, code, now_ns()) {
            EngineDecision::RestartAt { attempt, .. } => self.pending_attempt = attempt,
            EngineDecision::Blocked => {
                debug_write_bytes(b"init: crash-loop blocked svc=fault-probe reason=");
                debug_write_bytes(reason.label().as_bytes());
                debug_write_byte(b'\n');
                if self.restart_ok_emitted {
                    debug_write_bytes(b"SELFTEST: crash-loop cap ok\n");
                } else {
                    // Cap without a proven restart means the schedule never
                    // ran — loud, never silently green.
                    debug_write_bytes(b"SELFTEST: crash-loop cap FAIL\n");
                }
            }
            EngineDecision::Rest => {
                // A fault probe resting means the reason arrived as clean —
                // kernel truth broke; say so.
                debug_write_bytes(b"SELFTEST: supervision restart FAIL\n");
            }
        }
    }

    /// Respawn when the scheduled backoff is due (called once per round).
    fn tick(&mut self) {
        if self.pid.is_some() || !self.child.restart_due(now_ns()) {
            return;
        }
        debug_write_bytes(b"init: restart svc=fault-probe attempt=0x");
        debug_write_hex(self.pending_attempt as usize);
        debug_write_byte(b'\n');
        match spawn_fault_probe() {
            Some(pid) => {
                self.pid = Some(pid);
                self.child.on_restarted();
            }
            None => {
                debug_write_bytes(b"init: FAIL fault-probe respawn\n");
                // Ack the schedule so a permanently failing spawn cannot
                // busy-respawn; the missing selftest markers stay the alarm.
                self.child.on_restarted();
            }
        }
    }
}

/// Monotonic now for engine decisions; `nsec` is the boot-proven time
/// source everywhere on the OS path.
fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// Spawns one `demo.fault` probe child (suspended-spawn + resume, #102).
fn spawn_fault_probe() -> Option<u32> {
    match nexus_abi::exec(demo_exit0::DEMO_FAULT_ELF, 4, 0) {
        Ok(pid) => {
            if nexus_abi::task_resume(pid).is_err() {
                debug_write_bytes(b"init: FAIL fault-probe resume\n");
                None
            } else {
                Some(pid)
            }
        }
        Err(_) => {
            debug_write_bytes(b"init: FAIL fault-probe spawn\n");
            None
        }
    }
}

/// `init: service exit name=<svc> reason=<label> code=0x<hex>` — the
/// kernel-truth death line for a boot service (RFC-0087 §2). `name=unknown`
/// for a child without a control channel (nothing supervised is nameless;
/// if this ever prints, that is the finding).
fn announce_service_exit(
    pid: u32,
    code: i32,
    reason: ExitReason,
    channels: &[CtrlChannel],
    route_table: &mut crate::route_table::RouteTable,
) {
    let name =
        channels.iter().find(|chan| chan.pid == pid).map(|chan| chan.svc_name).unwrap_or("unknown");
    // ADR-0057: the moment a service is known dead, its routes answer STALE
    // instead of handing out slots that dangle on the corpse. Cleared when a
    // restarted instance is re-provisioned (PR-B3b).
    if let Some(id) = crate::service_topology::ServiceId::from_name(name.as_bytes()) {
        route_table.mark_stale(id);
    }
    debug_write_bytes(b"init: service exit name=");
    debug_write_bytes(name.as_bytes());
    debug_write_bytes(b" reason=");
    debug_write_bytes(reason.label().as_bytes());
    debug_write_bytes(b" code=0x");
    debug_write_hex(code as u32 as usize);
    debug_write_byte(b'\n');
}
