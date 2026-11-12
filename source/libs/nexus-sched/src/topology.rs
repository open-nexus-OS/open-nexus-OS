// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the machine as a program sees it (RFC-0118) — how many logical CPUs the kernel
//! runs, which are online, which share a cluster — and the one operation of the sched syscall
//! that says so. A program asks; it never parses the device tree for this and never assumes a
//! count. The numbers here are the wire: the kernel answers with them, `nexus-abi` asks with
//! them. Also the placement roles of ADR-0049, derived from the picture instead of written as
//! masks: nothing names a CPU by number.
//! OWNERS: @kernel-team @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (wire: `(TOPOLOGY_OP, what, index)` in, one register out)
//! TEST_COVERAGE: `tests` below (the picture from answers, refused pictures, the derived
//!   roles on one, two and eight CPUs); the kernel's answers are tested against this reader in
//!   `neuron::topology`
//! RFC: docs/rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md

use crate::CpuSet;

/// The sched syscall's topology operation: `args = (TOPOLOGY_OP, what, index)`, registers
/// only — no user memory is read or written. Any task may ask: the answer carries no
/// authority.
pub const TOPOLOGY_OP: usize = 7;

/// What a topology query asks (`what`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum TopologyQuery {
    /// The number of logical CPUs the kernel runs.
    Cpus = 0,
    /// The [`CpuSet`] of CPUs that are online — the set a task's CPU set is admitted against.
    Online = 1,
    /// The number of clusters (1 when the machine names none).
    Clusters = 2,
    /// The [`CpuSet`] of cluster `index`; refused beyond the count.
    ClusterSet = 3,
    /// The hart id of CPU `index` (for diagnostics and logs); refused beyond the count.
    HartOf = 4,
}

impl TopologyQuery {
    /// The query a register names.
    #[must_use]
    pub const fn from_raw(raw: usize) -> Option<Self> {
        Some(match raw {
            0 => Self::Cpus,
            1 => Self::Online,
            2 => Self::Clusters,
            3 => Self::ClusterSet,
            4 => Self::HartOf,
            _ => return None,
        })
    }
}

/// Where a program runs, by role (ADR-0049) — until roles become time rights (TASK-0365).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// The display and input chain: the boot hart's CPU.
    Display,
    /// Everything else: every online CPU but the display chain's — all of them on a machine
    /// of one.
    Background,
    /// No placement: every online CPU.
    Any,
}

/// The machine's logical CPUs as one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topology {
    cpus: usize,
    online: CpuSet,
    clusters: usize,
    cluster_sets: [CpuSet; CpuSet::BITS],
}

impl Topology {
    /// Builds the picture from answers: `ask(what, index)` is one topology query, `None` a
    /// refused one. `None` when an answer is refused or the answers do not describe a machine
    /// (no CPU, more than a set names, an online CPU beyond the count, no online CPU, no
    /// cluster). A machine naming more clusters than a set has bits yields the first of them.
    pub fn from_queries(
        mut ask: impl FnMut(TopologyQuery, usize) -> Option<usize>,
    ) -> Option<Self> {
        let cpus = ask(TopologyQuery::Cpus, 0)?;
        let online = CpuSet::from_bits(ask(TopologyQuery::Online, 0)? as u64);
        let clusters = ask(TopologyQuery::Clusters, 0)?.min(CpuSet::BITS);
        let beyond = online.without(CpuSet::first(cpus));
        if cpus == 0 || cpus > CpuSet::BITS || online.is_empty() || !beyond.is_empty() {
            return None;
        }
        if clusters == 0 {
            return None;
        }
        let mut cluster_sets = [CpuSet::EMPTY; CpuSet::BITS];
        for (index, set) in cluster_sets.iter_mut().enumerate().take(clusters) {
            *set = CpuSet::from_bits(ask(TopologyQuery::ClusterSet, index)? as u64);
        }
        Some(Self { cpus, online, clusters, cluster_sets })
    }

    /// The number of logical CPUs the kernel runs.
    #[must_use]
    pub const fn cpus(&self) -> usize {
        self.cpus
    }

    /// The CPUs that are online.
    #[must_use]
    pub const fn online(&self) -> CpuSet {
        self.online
    }

    /// The number of clusters.
    #[must_use]
    pub const fn clusters(&self) -> usize {
        self.clusters
    }

    /// The CPUs of cluster `cluster`; `None` beyond the count.
    #[must_use]
    pub fn cluster_set(&self, cluster: usize) -> Option<CpuSet> {
        self.cluster_sets.get(..self.clusters)?.get(cluster).copied()
    }

    /// The CPU set of a role on this machine — always a set the kernel admits.
    #[must_use]
    pub const fn set_for(&self, placement: Placement) -> CpuSet {
        placement.set(self.online)
    }
}

impl Placement {
    /// The CPU set of this role given the online CPUs (CPU 0, the boot hart, is always one of
    /// them). Never empty for a non-empty `online`.
    #[must_use]
    pub const fn set(self, online: CpuSet) -> CpuSet {
        let display = online.intersection(CpuSet::first(1));
        let rest = online.without(display);
        match self {
            Self::Any => online,
            Self::Display if !display.is_empty() => display,
            Self::Background if !rest.is_empty() => rest,
            Self::Display | Self::Background => online,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kernel of `cpus` CPUs, `online` of them up, in the given clusters.
    fn kernel(
        cpus: usize,
        online: u64,
        clusters: &[u64],
    ) -> impl FnMut(TopologyQuery, usize) -> Option<usize> + '_ {
        move |what, index| match what {
            TopologyQuery::Cpus => Some(cpus),
            TopologyQuery::Online => Some(online as usize),
            TopologyQuery::Clusters => Some(clusters.len()),
            TopologyQuery::ClusterSet => clusters.get(index).map(|&set| set as usize),
            TopologyQuery::HartOf => (index < cpus).then_some(index),
        }
    }

    #[test]
    fn the_picture_is_what_the_kernel_answered() {
        let machine = Topology::from_queries(kernel(8, 0xbf, &[0x0f, 0xf0])).unwrap();
        assert_eq!((machine.cpus(), machine.clusters()), (8, 2));
        assert_eq!(machine.online(), CpuSet::from_bits(0xbf));
        assert_eq!(machine.cluster_set(1), Some(CpuSet::from_bits(0xf0)));
    }

    #[test]
    fn test_reject_topology_index_beyond_count() {
        let machine = Topology::from_queries(kernel(4, 0xf, &[0xf])).unwrap();
        assert_eq!(machine.cluster_set(1), None);
        assert_eq!(machine.cluster_set(usize::MAX), None);
        assert_eq!(TopologyQuery::from_raw(5), None);
    }

    #[test]
    fn test_reject_answers_that_describe_no_machine() {
        // No CPU, more CPUs than a set names, nobody online, an online CPU beyond the count,
        // no cluster, a cluster the kernel then refuses.
        assert_eq!(Topology::from_queries(kernel(0, 0x1, &[0x1])), None);
        assert_eq!(Topology::from_queries(kernel(65, 0x1, &[0x1])), None);
        assert_eq!(Topology::from_queries(kernel(4, 0x0, &[0xf])), None);
        assert_eq!(Topology::from_queries(kernel(4, 0x10, &[0xf])), None);
        assert_eq!(Topology::from_queries(kernel(4, 0xf, &[])), None);
        let mut refusing = |what, index| match what {
            TopologyQuery::ClusterSet => None,
            _ => kernel(4, 0xf, &[0xf])(what, index),
        };
        assert_eq!(Topology::from_queries(&mut refusing), None);
        assert_eq!(Topology::from_queries(|_, _| None), None);
    }

    #[test]
    fn roles_are_derived_from_the_machine_never_written_as_masks() {
        let eight = Topology::from_queries(kernel(8, 0xff, &[0x0f, 0xf0])).unwrap();
        assert_eq!(eight.set_for(Placement::Display).bits(), 0x01);
        assert_eq!(eight.set_for(Placement::Background).bits(), 0xfe);
        assert_eq!(eight.set_for(Placement::Any).bits(), 0xff);
        // Two CPUs: the background has the other one.
        let two = Topology::from_queries(kernel(2, 0x3, &[0x3])).unwrap();
        assert_eq!(two.set_for(Placement::Background).bits(), 0x2);
        // One CPU: every role is that CPU — a set the kernel admits, not an empty one.
        let one = Topology::from_queries(kernel(1, 0x1, &[0x1])).unwrap();
        for role in [Placement::Display, Placement::Background, Placement::Any] {
            assert_eq!(one.set_for(role).bits(), 0x1);
        }
        // A machine that lost CPUs 1 and 2 at bring-up: the background is what is left.
        let lost = Topology::from_queries(kernel(4, 0b1001, &[0xf])).unwrap();
        assert_eq!(lost.set_for(Placement::Background).bits(), 0b1000);
    }

    #[test]
    fn every_role_names_an_online_cpu_on_every_machine() {
        for cpus in 1..=CpuSet::BITS {
            let online = CpuSet::first(cpus);
            for role in [Placement::Display, Placement::Background, Placement::Any] {
                let set = role.set(online);
                assert!(set.admit(online, online).is_ok(), "{role:?} on {cpus}");
            }
        }
    }
}
