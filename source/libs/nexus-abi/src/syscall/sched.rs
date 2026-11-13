// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the sched syscall's operations (B, TASK-0042) — a task's CPU set and shares
//! (target 0 = self, cross-task requires the QoS-admin capability), the machine's topology
//! (RFC-0118), what its CPUs offer a program (RFC-0119), and the kernel's budget and
//! baseline reports. A CPU set is a [`CpuSet`] on
//! both sides of the syscall; a program asks the kernel for the machine and derives its sets
//! from the answer — it never assumes a count and never writes a mask.
//! (Split out of `task.rs` under the structure ratchet; syscall ID and operations unchanged.)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `SELFTEST: affinity applied ok` (pin, a set beyond the harts refused, widen
//!   back), `SELFTEST: workpool affinity ok`; the set and topology rules are host-tested in
//!   `nexus-sched`
//! RFC: docs/rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md

use super::*;
pub use nexus_sched::{CpuSet, Extension, Offers, Placement, Topology};
use nexus_sched::{TopologyQuery, TOPOLOGY_OP};

const SYSCALL_SCHED: usize = 48;
const OP_GET_AFFINITY: usize = 0;
const OP_SET_AFFINITY: usize = 1;
const OP_GET_SHARES: usize = 2;
const OP_SET_SHARES: usize = 3;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn op(op: usize, target: usize, value: usize) -> SysResult<usize> {
    // SAFETY: plain syscall; the kernel validates every argument.
    let raw = unsafe { ecall3(SYSCALL_SCHED, op, target, value) };
    decode_syscall(raw)
}

/// One topology query (RFC-0118): `(what, index)` in registers, one register back; a
/// refusal is an index beyond its count.
fn ask(what: TopologyQuery, index: usize) -> SysResult<usize> {
    op(TOPOLOGY_OP, what as usize, index)
}

/// The machine's logical CPUs as one value: how many the kernel runs, which are online,
/// which share a cluster. A program derives its CPU sets from this (`Topology::set_for`)
/// — it never assumes a count and never names a CPU by number.
#[must_use = "the topology is what a CPU set is derived from"]
pub fn topology() -> SysResult<Topology> {
    Topology::from_queries(|what, index| ask(what, index).ok()).ok_or(AbiError::Unsupported)
}

/// The CPUs that are online — the set a CPU set is admitted against (one query).
#[must_use = "the online set is what a CPU set is derived from"]
pub fn online() -> SysResult<CpuSet> {
    ask(TopologyQuery::Online, 0).map(|bits| CpuSet::from_bits(bits as u64))
}

/// The hart id of logical CPU `cpu`, for diagnostics and logs — never an index.
#[must_use = "hart lookup result must be handled"]
pub fn hart_of(cpu: usize) -> SysResult<u64> {
    ask(TopologyQuery::HartOf, cpu).map(|hart| hart as u64)
}

/// What every CPU offers this program (RFC-0119): the extensions a thread may use on
/// whichever CPU it runs, and the vector registers' width. A program chooses its code path
/// from this — it never assumes an extension and never executes one to find out. A word that
/// contradicts itself is refused like no answer.
#[must_use = "what the CPUs offer is what a code path is chosen from"]
pub fn offers() -> SysResult<Offers> {
    ask(TopologyQuery::Offers, 0).and_then(|raw| Offers::from_raw(raw).ok_or(AbiError::Unsupported))
}

/// Returns the caller's CPU set (bit N = may run on logical CPU N).
#[must_use = "affinity get result must be handled"]
pub fn get_affinity() -> SysResult<CpuSet> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        op(OP_GET_AFFINITY, 0, 0).map(|bits| CpuSet::from_bits(bits as u64))
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Sets the caller's CPU set. The kernel refuses (never clamps) a set that is empty,
/// names a CPU it does not run, or names no online CPU — derive the set from
/// [`topology`] and none of these can happen.
#[must_use = "affinity set result must be handled"]
pub fn set_affinity(set: CpuSet) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        op(OP_SET_AFFINITY, 0, set.bits() as usize).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = set;
        Err(AbiError::Unsupported)
    }
}

/// Returns the caller's scheduling shares [1, 1000].
#[must_use = "shares get result must be handled"]
pub fn get_shares() -> SysResult<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        op(OP_GET_SHARES, 0, 0)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Sets the caller's scheduling shares; the kernel clamps to [1, 1000]
/// and returns the applied value.
#[must_use = "shares set result must be handled"]
pub fn set_shares(shares: usize) -> SysResult<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        op(OP_SET_SHARES, 0, shares)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = shares;
        Err(AbiError::Unsupported)
    }
}

/// Emit the kernel's BKL budget gate line (P0; late in the boot ladder
/// so the report covers the bring-up contention window).
pub fn bkl_budget_report() {
    let _ = op(4, 0, 0);
}

/// Log the bring-up burst maxima and reset the accounting (P0 two-window:
/// called once bring-up completes; the gate then judges steady state).
pub fn bkl_budget_reset() {
    let _ = op(5, 0, 0);
}

/// Print the kernel's baseline lines (where the lock's time went, how long runnable tasks
/// waited per class, who yields) for the window ending now, and begin a new window.
pub fn baseline_window() {
    let _ = op(6, 0, 0);
}

/// Cross-task affinity (B4: execd applies declarative sched recipes).
/// Requires QoS-admin standing in the kernel (execd/policyd).
#[must_use = "sched outcomes must be handled"]
pub fn set_affinity_for(pid: u32, set: CpuSet) -> SysResult<()> {
    op(OP_SET_AFFINITY, pid as usize, set.bits() as usize).map(|_| ())
}

/// Cross-task shares (B4).
#[must_use = "sched outcomes must be handled"]
pub fn set_shares_for(pid: u32, shares: usize) -> SysResult<()> {
    op(OP_SET_SHARES, pid as usize, shares).map(|_| ())
}
