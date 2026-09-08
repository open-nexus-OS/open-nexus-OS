// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Supervision policy SSOT (TASK-0049B / RFC-0087 §2) — criticality
//! tier + restart policy for every supervised boot service, plus the ONE
//! declared backoff schedule. A sibling to `service_topology`'s route SSOT,
//! deliberately NOT `ServiceSpec` fields: half the fleet is bespoke-wired
//! and has no spec (and must never grow one, or the generic provisioning
//! arm would fire for it). Consumed by the responder's supervision sweep;
//! the restart/backoff engine (PR-B2) decides from this table and the
//! ADR-0056 exit reason — never from the exit code alone.
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host tests below (coverage/tier/backoff contract)
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md

use crate::service_topology::ServiceId;

/// RFC-0087 §2 criticality tiers — how hard the system fights for a service
/// and what its death means (TASK-0049B).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Criticality {
    /// Boot cannot be healthy without it; runtime death restarts with the
    /// highest urgency and repeated failure escalates (reboot-into-safe once
    /// TASK-0050 ships the reset path).
    CriticalBoot,
    /// The session degrades visibly without it; restart + client re-resolve.
    CriticalSession,
    /// Restart per policy; a crash loop parks the service without harming
    /// the system.
    Standard,
}

/// RFC-0087 §2 restart policy. Decisions consume the REAL exit reason
/// (ADR-0056) — a `clean` exit NEVER counts toward a crash loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Log the exit, leave the service down.
    Never,
    /// Restart on `fault`/`killed`/`error`, not on `clean`.
    OnFailure,
    /// Restart on every exit (clean included).
    Always,
}

/// RFC-0087 §2 backoff parameters (deterministic; injectable time source in
/// tests). ONE declared default for all services — a per-service override
/// earns its place in this table when a service demonstrably needs one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackoffSpec {
    /// First restart delay in milliseconds.
    pub initial_ms: u32,
    /// Multiplier per subsequent restart.
    pub factor: u32,
    /// Delay ceiling in milliseconds.
    pub max_ms: u32,
    /// Crash-loop window in seconds.
    pub window_s: u32,
    /// Crashes within the window before the service is parked (`blocked`).
    pub threshold: u8,
}

/// The declared default backoff schedule (500 → 1000 → 2000 → … clamped to
/// 15 s; 5 crashes in 60 s parks the service).
pub const DEFAULT_BACKOFF: BackoffSpec =
    BackoffSpec { initial_ms: 500, factor: 2, max_ms: 15_000, window_s: 60, threshold: 5 };

/// Supervision SSOT (TASK-0049B): criticality tier + restart policy for
/// EVERY supervised boot service — deliberately a sibling table to
/// [`SERVICE_SPECS`] rather than spec fields, because half the fleet is
/// bespoke-wired and has no `ServiceSpec` (and must never grow one, or the
/// generic provisioning arm would fire for it). Pseudo ids (`ImedOsk`) and
/// the harness (`SelftestClient`) are intentionally absent; the
/// completeness test below pins the covered set.
pub const SUPERVISION: &[(ServiceId, Criticality, RestartPolicy)] = &[
    // RFC-0087 critical-boot set: statefsd, samgrd, policyd, execd, logd.
    (ServiceId::Statefsd, Criticality::CriticalBoot, RestartPolicy::Always),
    // TASK-0315: the storage substrate under statefs AND nxfs.
    (ServiceId::Virtioblkd, Criticality::CriticalBoot, RestartPolicy::Always),
    (ServiceId::Samgrd, Criticality::CriticalBoot, RestartPolicy::Always),
    (ServiceId::Policyd, Criticality::CriticalBoot, RestartPolicy::Always),
    (ServiceId::Execd, Criticality::CriticalBoot, RestartPolicy::Always),
    (ServiceId::Logd, Criticality::CriticalBoot, RestartPolicy::Always),
    (ServiceId::Keystored, Criticality::CriticalBoot, RestartPolicy::Always),
    // Session tier: the desktop degrades visibly when these die.
    (ServiceId::Windowd, Criticality::CriticalSession, RestartPolicy::OnFailure),
    (ServiceId::Gpud, Criticality::CriticalSession, RestartPolicy::OnFailure),
    (ServiceId::Vfsd, Criticality::CriticalSession, RestartPolicy::OnFailure),
    (ServiceId::Inputd, Criticality::CriticalSession, RestartPolicy::OnFailure),
    (ServiceId::Settingsd, Criticality::CriticalSession, RestartPolicy::OnFailure),
    (ServiceId::Sessiond, Criticality::CriticalSession, RestartPolicy::OnFailure),
    (ServiceId::Bundlemgrd, Criticality::CriticalSession, RestartPolicy::OnFailure),
    (ServiceId::Abilitymgr, Criticality::CriticalSession, RestartPolicy::OnFailure),
    (ServiceId::Packagefsd, Criticality::CriticalSession, RestartPolicy::OnFailure),
    // Boot-state authority (TASK-0050, ADR-0055): losing the record owner
    // mid-boot dead-ends every OTA/target decision — critical-boot tier.
    (ServiceId::Bootctld, Criticality::CriticalBoot, RestartPolicy::Always),
    // Standard tier.
    (ServiceId::Updated, Criticality::Standard, RestartPolicy::OnFailure),
    (ServiceId::Rngd, Criticality::Standard, RestartPolicy::OnFailure),
    (ServiceId::Timed, Criticality::Standard, RestartPolicy::OnFailure),
    (ServiceId::Imed, Criticality::Standard, RestartPolicy::OnFailure),
    (ServiceId::Netstackd, Criticality::Standard, RestartPolicy::OnFailure),
    // RFC-0092 / TASK-0052: the inbound gateway follows the stack it fronts.
    (ServiceId::Ingressd, Criticality::Standard, RestartPolicy::OnFailure),
    (ServiceId::Dsoftbusd, Criticality::Standard, RestartPolicy::OnFailure),
    (ServiceId::Metricsd, Criticality::Standard, RestartPolicy::OnFailure),
    (ServiceId::Hidrawd, Criticality::Standard, RestartPolicy::OnFailure),
    (ServiceId::Touchd, Criticality::Standard, RestartPolicy::OnFailure),
    (ServiceId::Pinched, Criticality::Standard, RestartPolicy::OnFailure),
];

/// Supervision entry for a service, if it is a supervised boot service.
pub fn supervision_for(id: ServiceId) -> Option<(Criticality, RestartPolicy)> {
    SUPERVISION.iter().find(|(sid, _, _)| *sid == id).map(|(_, c, r)| (*c, *r))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TASK-0049B: the supervision SSOT covers every supervised boot service
    /// exactly once — pseudo ids and the harness stay out. A new ServiceId
    /// without a supervision entry is a host-test failure, not a silent
    /// unsupervised service.
    #[test]
    fn supervision_covers_every_boot_service_exactly_once() {
        for (i, (a, _, _)) in SUPERVISION.iter().enumerate() {
            for (b, _, _) in &SUPERVISION[i + 1..] {
                assert_ne!(a, b, "duplicate supervision entry {a:?}");
            }
        }
        for id in ServiceId::ALL {
            let expected_absent = matches!(id, ServiceId::ImedOsk | ServiceId::SelftestClient);
            assert_eq!(
                supervision_for(id).is_none(),
                expected_absent,
                "supervision coverage wrong for {id:?}"
            );
        }
    }

    /// RFC-0087 §2: the critical-boot tier restarts unconditionally, and a
    /// clean-exit-tolerant policy on that tier would be a contradiction.
    #[test]
    fn critical_boot_services_always_restart() {
        for (id, crit, restart) in SUPERVISION {
            if *crit == Criticality::CriticalBoot {
                assert_eq!(
                    *restart,
                    RestartPolicy::Always,
                    "critical-boot service {id:?} must restart always"
                );
            }
        }
        // The RFC-0087 §2 named floor is present and correctly tiered.
        for id in [
            ServiceId::Statefsd,
            ServiceId::Samgrd,
            ServiceId::Policyd,
            ServiceId::Execd,
            ServiceId::Logd,
        ] {
            assert_eq!(
                supervision_for(id).map(|(c, _)| c),
                Some(Criticality::CriticalBoot),
                "{id:?} must be critical-boot per RFC-0087"
            );
        }
    }

    /// The declared default backoff is the RFC-0087 schedule shape:
    /// 500 → 1000 → … clamped at 15 s, 5-in-60s parks.
    #[test]
    fn default_backoff_matches_contract() {
        assert_eq!(DEFAULT_BACKOFF.initial_ms, 500);
        assert_eq!(DEFAULT_BACKOFF.factor, 2);
        assert_eq!(DEFAULT_BACKOFF.max_ms, 15_000);
        assert_eq!(DEFAULT_BACKOFF.window_s, 60);
        assert_eq!(DEFAULT_BACKOFF.threshold, 5);
    }
}
