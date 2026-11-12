// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the logical CPUs a kernel runs, as the tree states them (RFC-0118): CPU 0 is the
//! boot hart whatever its hart id, the others follow in the tree's order of enabled
//! `/cpus/cpu@*` nodes whose hart has an MMU; each carries its hart id, its cluster
//! (`/cpus/cpu-map`, one cluster when the tree names none) and the PLIC context of its
//! supervisor external interrupt (the controller's `interrupts-extended`, never
//! `2·hart + 1`). A value of fixed capacity `N` — the reader has no heap; a tree naming more
//! harts than `N` yields the first `N` and says how many it named. Nothing here knows a
//! machine: hart ids are as wide and as sparse as the tree states, the map nests as the
//! binding allows.

use crate::header::{Error, Fdt};

/// One logical CPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicalCpu {
    /// The hart id (`reg` of the cpu node, as wide as the tree states it) — for SBI calls
    /// and logs, never an index.
    pub hart: u64,
    /// The cluster index (tree order of the innermost `cluster*` nodes of `/cpus/cpu-map`);
    /// 0 when the tree names no map or the map does not name this cpu.
    pub cluster: u32,
    /// The PLIC context of this hart's supervisor external interrupt, when a PLIC names one.
    pub plic_context: Option<u32>,
}

/// The logical CPUs of a machine, at most `N`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topology<const N: usize> {
    cpus: [LogicalCpu; N],
    count: usize,
    named: usize,
    runnable: usize,
    clusters: usize,
}

impl<const N: usize> Topology<N> {
    const NONE: LogicalCpu = LogicalCpu { hart: 0, cluster: 0, plic_context: None };

    /// The machine of one hart the tree did not describe: `boot_hart` alone, one cluster.
    /// What a kernel runs when the tree's `/cpus` cannot be read — it is on that hart already.
    #[must_use]
    pub fn single(boot_hart: u64) -> Self {
        let mut cpus = [Self::NONE; N];
        let mut count = 0;
        if let Some(slot) = cpus.first_mut() {
            slot.hart = boot_hart;
            count = 1;
        }
        Self { cpus, count, named: 1, runnable: 1, clusters: 1 }
    }

    /// Reads the topology with `boot_hart` as CPU 0. A logical CPU is an enabled cpu node
    /// whose hart has an MMU (`Cpu::has_mmu`). Refused when the tree has no `/cpus` or does
    /// not name the boot hart among those (`Error::NotFound`): a tree that does not describe
    /// the hart it was handed over on is not this machine's.
    pub fn read(fdt: &Fdt<'_>, boot_hart: u64) -> Result<Self, Error> {
        let tree = fdt.cpus()?;
        let map = tree.cpu_map();
        let plic = fdt.plic();
        let describe = |cpu: &crate::platform::Cpu<'_>| LogicalCpu {
            hart: cpu.hart,
            cluster: map
                .zip(cpu.phandle())
                .and_then(|(map, phandle)| map.cluster_of(phandle))
                .unwrap_or(0),
            plic_context: plic
                .and_then(|p| p.plic_s_contexts().find(|(hart, _)| *hart == cpu.hart))
                .map(|(_, context)| context),
        };
        let runs = || tree.harts().filter(|c| c.has_mmu());
        let boot = runs().find(|c| c.hart == boot_hart).ok_or(Error::NotFound)?;
        let mut cpus = [Self::NONE; N];
        let mut count = 0;
        let rest = runs().filter(|c| c.hart != boot_hart);
        for cpu in core::iter::once(boot).chain(rest) {
            let Some(slot) = cpus.get_mut(count) else { break };
            *slot = describe(&cpu);
            count += 1;
        }
        let clusters = map.map_or(1, |m| m.clusters()).max(1);
        Ok(Self { cpus, count, named: tree.count(), runnable: runs().count(), clusters })
    }

    /// The number of logical CPUs (at most `N`, at least 1 for a tree that was read).
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// The number of enabled harts the tree names.
    #[must_use]
    pub fn named(&self) -> usize {
        self.named
    }

    /// The number of those with an MMU — above [`Self::count`] when the tree names more than
    /// `N`, below [`Self::named`] when it names harts an OS cannot schedule on.
    #[must_use]
    pub fn runnable(&self) -> usize {
        self.runnable
    }

    /// The number of clusters (1 when the tree names none).
    #[must_use]
    pub fn clusters(&self) -> usize {
        self.clusters
    }

    /// Logical CPU `cpu`.
    #[must_use]
    pub fn cpu(&self, cpu: usize) -> Option<LogicalCpu> {
        self.cpus.get(..self.count)?.get(cpu).copied()
    }

    /// Every logical CPU, in index order.
    pub fn cpus(&self) -> impl Iterator<Item = LogicalCpu> + '_ {
        self.cpus.iter().take(self.count).copied()
    }

    /// The hart id of logical CPU `cpu`.
    #[must_use]
    pub fn hart_of(&self, cpu: usize) -> Option<u64> {
        self.cpu(cpu).map(|c| c.hart)
    }

    /// The logical CPU that is hart `hart`.
    #[must_use]
    pub fn cpu_of(&self, hart: u64) -> Option<usize> {
        self.cpus().position(|c| c.hart == hart)
    }

    /// The CPUs of cluster `cluster` as a bit set (bit `n` = logical CPU `n`, the first 64);
    /// `None` beyond the cluster count.
    #[must_use]
    pub fn cluster_bits(&self, cluster: usize) -> Option<u64> {
        if cluster >= self.clusters {
            return None;
        }
        let bits = self.cpus().take(64).enumerate();
        Some(
            bits.filter(|(_, c)| c.cluster as usize == cluster)
                .fold(0, |set, (n, _)| set | (1 << n)),
        )
    }
}
