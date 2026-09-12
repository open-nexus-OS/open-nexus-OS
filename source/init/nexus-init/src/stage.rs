// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: init's boot-stage ladder (ADR-0062, RFC-0093 §3) — the pure, host-tested decision
//! of WHEN a stage may be signalled on the kernel timeline fence. A stage is reached because
//! its members announced `@ready` (and, for the display stages, because windowd said so), never
//! because init resumed something, yielded, or waited out a timer.
//!
//! The barrier sets derive from the boot graph, so a target that excludes a service also
//! excludes it from every barrier: recovery reaches `DisplayReady` WITHOUT a display instead of
//! hanging on a windowd that will never run.
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P5-b)
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (`cargo test -p nexus-init`)
//! ADR: docs/adr/0062-boot-stage-fence-and-readiness-barriers.md

use crate::boot_graph::{in_core, includes, BootGraph};
use crate::service_topology::{spec_for, ServiceId, Stage};

/// Whether `svc` gates `stage` under `graph`.
///
/// Three rules, each from already-declared truth rather than a maintained list:
///
/// 1. **It must run under this target.** A service the boot graph excludes can never announce,
///    so waiting for it would hang the very mode meant to rescue the system.
/// 2. **It must SERVE something.** A stage promises that a tier's services are usable, and only
///    a service that exposes a server has something to promise. A pure client (the proof
///    harness) and pure producers (the HID forwarder, the touch driver) announce nothing to wait
///    for — measured: they are exactly the specs with `exposes_server: false`, and exactly the
///    ones that never print `init: up`.
/// 3. **It must belong to the tier.** `Platform` is gated by the CORE graph — the floor recovery
///    itself stands on — NOT by every service in the platform tier: wave-2 services (networking,
///    telemetry, the compute broker) consume the platform, they do not constitute it. The
///    display stage is gated by exactly the services declared for it.
///
/// `SessionStart` and `ShellVisible` have no service barrier: the first is an init-side
/// milestone (boot state committed + registry populated), the second is windowd's own report.
pub fn gates(stage: Stage, svc: ServiceId, graph: BootGraph) -> bool {
    if !includes(graph, svc.name()) {
        return false;
    }
    let Some(spec) = spec_for(svc.name().as_bytes()) else {
        return false;
    };
    if !spec.exposes_server {
        return false;
    }
    match stage {
        Stage::Platform => in_core(svc.name()),
        Stage::DisplayReady => spec.stage == Stage::DisplayReady,
        Stage::SessionStart | Stage::ShellVisible => false,
    }
}

/// `true` if no service gates `stage` under `graph` — the barrier is empty and signals at once
/// (RFC-0093 §3: recovery reaches `DisplayReady` without a display).
pub fn barrier_is_empty(stage: Stage, graph: BootGraph) -> bool {
    !ServiceId::ALL.iter().any(|&svc| gates(stage, svc, graph))
}

/// The monotone stage ladder. Signalling is one-way: a stage once reached STAYS reached, even
/// if a member later dies — the supervisor restarts it and its route is re-resolved (ADR-0057),
/// but the fence the fleet already passed can never move backwards.
#[derive(Debug, Clone, Copy)]
pub struct StageLadder {
    graph: BootGraph,
    signalled: u64,
    display_acked: bool,
    shell_acked: bool,
    milestones: bool,
}

impl StageLadder {
    /// A ladder at value 0 — nothing signalled yet.
    pub const fn new(graph: BootGraph) -> Self {
        Self { graph, signalled: 0, display_acked: false, shell_acked: false, milestones: false }
    }

    /// The highest fence value signalled so far.
    pub const fn signalled(&self) -> u64 {
        self.signalled
    }

    /// windowd reported a display stage (`@stage <label>`). Any other sender is refused by the
    /// responder before this is reached — identity is the control channel, never the payload.
    pub fn record_stage_report(&mut self, stage: Stage) {
        match stage {
            Stage::DisplayReady => self.display_acked = true,
            Stage::ShellVisible => self.shell_acked = true,
            // The platform floor and the session start are init's own to decide; a report
            // about them carries no information and is deliberately ignored rather than
            // trusted.
            Stage::Platform | Stage::SessionStart => {}
        }
    }

    /// init committed the boot state and populated the registry — the `SessionStart` milestone.
    pub fn record_milestones(&mut self) {
        self.milestones = true;
    }

    /// Whether `stage` may be signalled now. `is_ready` answers "has this service announced
    /// `@ready` and not exited since" (init's ready table, keyed by the pid of that service).
    pub fn satisfied(&self, stage: Stage, is_ready: &impl Fn(ServiceId) -> bool) -> bool {
        let members_ready = ServiceId::ALL
            .iter()
            .filter(|&&svc| gates(stage, svc, self.graph))
            .all(|&svc| is_ready(svc));
        match stage {
            Stage::Platform => members_ready,
            Stage::DisplayReady => {
                barrier_is_empty(stage, self.graph) || (members_ready && self.display_acked)
            }
            Stage::SessionStart => self.milestones,
            // Nothing to present without a compositor: a graph without windowd reaches the
            // last stage by construction, so anything gated on it still runs.
            Stage::ShellVisible => {
                !includes(self.graph, ServiceId::Windowd.name()) || self.shell_acked
            }
        }
    }

    /// The next stage to signal, if its barrier is satisfied. Advances ONE step per call (the
    /// caller loops), so every stage is signalled and marked in order — the fence value can
    /// never skip a rung.
    pub fn try_advance(&mut self, is_ready: &impl Fn(ServiceId) -> bool) -> Option<Stage> {
        let next = Stage::ALL.into_iter().find(|s| s.value() > self.signalled)?;
        if !self.satisfied(next, is_ready) {
            return None;
        }
        self.signalled = next.value();
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRAPHS: [BootGraph; 3] = [BootGraph::Normal, BootGraph::Safe, BootGraph::Recovery];

    /// A barrier may never wait on a service the boot target does not run — that is a boot that
    /// hangs forever, in the exact mode (recovery) that exists to rescue the system.
    #[test]
    fn test_reject_stage_barrier_depends_on_excluded_service() {
        for graph in GRAPHS {
            for stage in Stage::ALL {
                for svc in ServiceId::ALL {
                    if gates(stage, svc, graph) {
                        assert!(
                            includes(graph, svc.name()),
                            "{:?} barrier waits on {} which {graph:?} never resumes",
                            stage,
                            svc.name()
                        );
                    }
                }
            }
        }
    }

    /// A service that GATES a stage must not itself wait FOR that stage: it would wait for a
    /// value only its own `@ready` can produce. This is the stage ladder's deadlock guard.
    #[test]
    fn test_reject_barrier_member_waits_for_its_own_stage() {
        for graph in GRAPHS {
            for stage in Stage::ALL {
                for svc in ServiceId::ALL {
                    if !gates(stage, svc, graph) {
                        continue;
                    }
                    let spec = spec_for(svc.name().as_bytes()).expect("gating service is declared");
                    assert!(
                        spec.stage <= stage,
                        "{} gates {:?} but waits for {:?} — a boot that cannot start itself",
                        svc.name(),
                        stage,
                        spec.stage
                    );
                }
            }
        }
    }

    /// A barrier may only wait on a service that SERVES something. This is not a style rule: the
    /// proof harness and the pure producers never announce readiness at all (measured — they are
    /// the only specs with `exposes_server: false`, and the only fleet members that never print
    /// `init: up`), so a barrier containing one would never open and the whole boot would stall
    /// behind a service that was never going to answer.
    #[test]
    fn test_reject_barrier_member_without_a_server() {
        for graph in GRAPHS {
            for stage in Stage::ALL {
                for svc in ServiceId::ALL {
                    if !gates(stage, svc, graph) {
                        continue;
                    }
                    let spec = spec_for(svc.name().as_bytes()).expect("gating service is declared");
                    assert!(
                        spec.exposes_server,
                        "{} gates {:?} but serves nothing — it never announces readiness",
                        svc.name(),
                        stage
                    );
                }
            }
        }
    }

    /// ...and the platform barrier must not be empty in a normal boot: a floor nothing gates is
    /// a barrier that opens before the platform exists.
    #[test]
    fn platform_barrier_is_not_vacuous() {
        assert!(
            !barrier_is_empty(Stage::Platform, BootGraph::Normal),
            "nothing gates the platform floor"
        );
        assert!(
            !barrier_is_empty(Stage::DisplayReady, BootGraph::Normal),
            "nothing gates the display stage in a normal boot"
        );
    }

    /// Every CORE service belongs to the platform tier: the floor cannot be declared above
    /// itself (see the deadlock guard above — this is the same invariant stated positively).
    #[test]
    fn core_services_are_platform_stage() {
        for svc in ServiceId::ALL {
            if !in_core(svc.name()) {
                continue;
            }
            let spec = spec_for(svc.name().as_bytes()).expect("core service is declared");
            assert_eq!(spec.stage, Stage::Platform, "core service {} is not platform", svc.name());
        }
    }

    /// The ladder climbs rung by rung and never skips, whatever order the evidence arrives in.
    #[test]
    fn ladder_advances_in_order_without_skipping() {
        let mut ladder = StageLadder::new(BootGraph::Normal);
        let nothing_ready = |_: ServiceId| false;
        assert_eq!(ladder.try_advance(&nothing_ready), None);

        // Evidence for the LATER stages first: the ladder must still not skip the platform.
        ladder.record_stage_report(Stage::DisplayReady);
        ladder.record_stage_report(Stage::ShellVisible);
        ladder.record_milestones();
        assert_eq!(ladder.try_advance(&nothing_ready), None, "platform skipped");

        let all_ready = |_: ServiceId| true;
        assert_eq!(ladder.try_advance(&all_ready), Some(Stage::Platform));
        assert_eq!(ladder.try_advance(&all_ready), Some(Stage::DisplayReady));
        assert_eq!(ladder.try_advance(&all_ready), Some(Stage::SessionStart));
        assert_eq!(ladder.try_advance(&all_ready), Some(Stage::ShellVisible));
        assert_eq!(ladder.try_advance(&all_ready), None, "ladder ran past its last rung");
        assert_eq!(ladder.signalled(), Stage::ShellVisible.value());
    }

    /// Recovery has no display: both display stages must still be reachable, or everything
    /// declared behind them is stranded in the mode meant to repair the system.
    #[test]
    fn recovery_reaches_every_stage_without_a_display() {
        let mut ladder = StageLadder::new(BootGraph::Recovery);
        ladder.record_milestones();
        let core_ready = |svc: ServiceId| in_core(svc.name());
        assert_eq!(ladder.try_advance(&core_ready), Some(Stage::Platform));
        assert_eq!(ladder.try_advance(&core_ready), Some(Stage::DisplayReady));
        assert_eq!(ladder.try_advance(&core_ready), Some(Stage::SessionStart));
        assert_eq!(ladder.try_advance(&core_ready), Some(Stage::ShellVisible));
    }

    /// A stage is not reached because init resumed something: without windowd's report the
    /// display stage stays shut even when every display service is ready.
    #[test]
    fn test_reject_display_stage_without_windowd_report() {
        let mut ladder = StageLadder::new(BootGraph::Normal);
        let all_ready = |_: ServiceId| true;
        assert_eq!(ladder.try_advance(&all_ready), Some(Stage::Platform));
        assert_eq!(ladder.try_advance(&all_ready), None, "display stage opened without a report");
        ladder.record_stage_report(Stage::DisplayReady);
        assert_eq!(ladder.try_advance(&all_ready), Some(Stage::DisplayReady));
    }
}
