// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the sched ABI applied from userspace (B6, TASK-0042; RFC-0118) — a task asks the
//!   kernel for the machine, pins itself to CPU 0, is refused a set naming a CPU the kernel
//!   does not run, and widens back to the online CPUs; a shares request persists exactly.
//!   Split out of `soaks.rs` (the architecture gate's size rule).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — `SELFTEST: affinity applied ok`,
//!   `SELFTEST: qos shares ratio ok`.
//! RFC: docs/rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md

use nexus_abi::sched::{self, CpuSet, Placement};

use crate::markers::emit_line;

pub(crate) fn sched_applied_proof() {
    // Affinity: pin to CPU 0 (the boot hart, on every machine), verify, widen back to the
    // CPUs the kernel reports online (asked, never a written mask) — and a set naming a CPU
    // the kernel does not run, beside a valid one, is refused and changes nothing.
    let cpu0 = CpuSet::first(1);
    let ok_aff = sched::topology().is_ok_and(|machine| {
        let beyond = CpuSet::single(machine.cpus()).map(|one| one.union(cpu0));
        sched::set_affinity(cpu0).is_ok()
            && sched::get_affinity() == Ok(cpu0)
            && beyond.is_none_or(|set| sched::set_affinity(set).is_err())
            && sched::get_affinity() == Ok(cpu0)
            && sched::set_affinity(machine.set_for(Placement::Any)).is_ok()
    });
    if ok_aff {
        emit_line(crate::markers::M_SELFTEST_AFFINITY_APPLIED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_AFFINITY_APPLIED_FAIL);
    }
    // Shares: a 3x ratio request must persist exactly (the slice math itself
    // is host-tested in the kernel; this proves the userspace path applies).
    let ok_shares = sched::set_shares(300).is_ok()
        && sched::get_shares() == Ok(300)
        && sched::set_shares(100).is_ok()
        && sched::get_shares() == Ok(100);
    if ok_shares {
        emit_line(crate::markers::M_SELFTEST_QOS_SHARES_RATIO_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_QOS_SHARES_RATIO_FAIL);
    }
}
