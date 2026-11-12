// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a task's CPU set at the kernel's edges (RFC-0118) — the admission at the ABI
//! boundary and the home CPU a set allows. The rules themselves are `nexus_sched::CpuSet`'s
//! (`admit`, `home`; host-tested there); this module binds them to the machine: the CPUs
//! the kernel runs are the topology's.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `nexus-sched` (`test_reject_empty_cpu_set`,
//!   `test_reject_cpu_set_beyond_the_harts`, `test_reject_cpu_set_without_an_online_cpu`);
//!   kernel selftests (sched affinity reject/clamp markers)
//! ADR: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md
//! RFC: docs/rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md

use nexus_sched::{CpuSet, CpuSetError};

use crate::types::CpuId;

/// Admits a set at the ABI boundary: it names at least one CPU, none the kernel does not
/// run, and at least one that is online (a set of only-offline CPUs would strand the task
/// forever). A refused set is refused as it is — never clamped into a valid one.
pub fn admit_affinity(set: CpuSet, online: CpuSet) -> Result<CpuSet, CpuSetError> {
    set.admit(crate::topology::runs(), online)
}

/// The home CPU a set allows: `home` when the set names it and it is online, otherwise the
/// lowest online CPU of the set (the boot hart as last resort — an admitted set always names
/// an online CPU).
pub fn clamp_home_to_affinity(set: CpuSet, home: CpuId, online: CpuSet) -> CpuId {
    set.home(home.as_index(), online).map_or(CpuId::BOOT, |cpu| CpuId::from_raw(cpu as u16))
}
