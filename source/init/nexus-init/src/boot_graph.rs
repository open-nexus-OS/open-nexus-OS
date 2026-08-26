// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: Boot-target service graphs (TASK-0050 PR-5, RFC-0087 §4).
//! A target selects WHICH of the fully provisioned services actually RUN:
//! init always spawns and wires the complete topology (identical mints,
//! identical slot layout — the positional contracts never shift), then
//! materializes the target as a RESUME SET. `recovery` resumes only the
//! core graph; `safe` is the session graph minus non-essential services;
//! `normal` resumes everything. Suspended services cost their provisioned
//! memory but never execute — an explicit trade against re-deriving the
//! slot layout per target, which is exactly the boot-layout drift class
//! this repo keeps paying for.
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host tests below (core ⊆ safe ⊆ normal, critical-boot
//!   tier always present, driver split honest).
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

/// Boot target as carried on the bootctld wire (0/1/2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootGraph {
    /// Full service set.
    Normal,
    /// Core-only ops graph (== wave 1).
    Recovery,
    /// Session graph minus non-essential services.
    Safe,
}

impl BootGraph {
    /// Maps the wire byte (bootctld target encoding); unknown = Normal is
    /// deliberately NOT offered — the caller must fail closed upstream.
    pub fn from_wire(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Normal),
            1 => Some(Self::Recovery),
            2 => Some(Self::Safe),
            _ => None,
        }
    }

    /// Marker label (`init: stage graph target=<label>`).
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Recovery => "recovery",
            Self::Safe => "safe",
        }
    }
}

/// The CORE graph: resumed in wave 1 on EVERY boot, before the target is
/// known — the boot-state authority and everything it needs to answer,
/// plus the storage/policy floor recovery operations stand on. This set
/// IS the recovery graph.
const CORE: &[&str] = &[
    // TASK-0315: the block plane underneath statefs — recovery ops (fsck,
    // record persistence) are dead without it.
    "virtioblkd",
    "statefsd",
    "logd",
    "policyd",
    "bootctld",
    "execd",
    "samgrd",
    "keystored",
    "rngd",
    "bundlemgrd",
    "packagefsd",
    "vfsd",
    // The proof harness is part of the image's topology; a shipped
    // recovery image simply would not contain it.
    "selftest-client",
];

/// Non-essential services withheld from the `safe` graph (session works,
/// networking/telemetry/compute-batch stay down).
const SAFE_EXCLUDED: &[&str] = &["netstackd", "dsoftbusd", "metricsd", "pinched"];

/// `true` if `service` is in wave 1 (the always-on core).
pub fn in_core(service: &str) -> bool {
    CORE.contains(&service)
}

/// `true` if `service` runs under `graph` (wave 2 + drivers consult this
/// AFTER the boot-attempt handshake resolved the one-shot target).
pub fn includes(graph: BootGraph, service: &str) -> bool {
    match graph {
        BootGraph::Normal => true,
        BootGraph::Recovery => in_core(service),
        BootGraph::Safe => !SAFE_EXCLUDED.contains(&service),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_supervision::{Criticality, SUPERVISION};
    use crate::service_topology::ServiceId;

    /// Every critical-boot service must run in EVERY graph — a target that
    /// drops the boot floor is a brick, not a mode.
    #[test]
    fn critical_boot_tier_present_in_all_graphs() {
        for (id, crit, _) in SUPERVISION {
            if *crit == Criticality::CriticalBoot {
                for graph in [BootGraph::Normal, BootGraph::Recovery, BootGraph::Safe] {
                    assert!(
                        includes(graph, id.name()),
                        "critical-boot {id:?} missing from {graph:?}"
                    );
                }
            }
        }
    }

    /// core ⊆ safe ⊆ normal — the graphs nest, they never fork.
    #[test]
    fn graphs_nest() {
        for id in &ServiceId::ALL {
            let name = id.name();
            if includes(BootGraph::Recovery, name) {
                assert!(includes(BootGraph::Safe, name), "{name} in recovery but not safe");
            }
            if includes(BootGraph::Safe, name) {
                assert!(includes(BootGraph::Normal, name), "{name} in safe but not normal");
            }
        }
    }

    /// Every core entry is a real service name (typo guard).
    #[test]
    fn core_names_resolve() {
        for name in CORE {
            assert!(
                ServiceId::from_name(name.as_bytes()).is_some(),
                "unknown core service `{name}`"
            );
        }
        for name in SAFE_EXCLUDED {
            assert!(
                ServiceId::from_name(name.as_bytes()).is_some(),
                "unknown safe-excluded service `{name}`"
            );
        }
    }

    #[test]
    fn wire_mapping_matches_bootctld() {
        assert_eq!(BootGraph::from_wire(0), Some(BootGraph::Normal));
        assert_eq!(BootGraph::from_wire(1), Some(BootGraph::Recovery));
        assert_eq!(BootGraph::from_wire(2), Some(BootGraph::Safe));
        assert_eq!(BootGraph::from_wire(7), None);
    }
}
