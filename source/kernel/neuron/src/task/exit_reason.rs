// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ADR-0056 exit-reason taxonomy — WHY a task died, recorded by the
//! kernel at the moment of death and delivered through `wait`/`wait_nohang`.
//! Split out of `task/mod.rs` (module-size ratchet).
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Unstable (wire encoding of `wire_bits` is the contract)
//! TEST_COVERAGE: exec-phase QEMU proof (`SELFTEST: exit reason ok`)
//! ADR: docs/adr/0056-task-exit-reason-kernel-abi.md

/// WHY a task died — kernel truth recorded at the moment of death and
/// delivered through `wait`/`wait_nohang` (ADR-0056). Userspace can never
/// set or mask it; `clean` vs `error` is derived from the exit code on the
/// consumer side, so the kernel only distinguishes HOW the exit happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    /// The task called `exit` itself (code carries clean/error semantics).
    Voluntary,
    /// The kernel killed the task on a trap; `cause` is the scause low byte
    /// (or a synthetic code ≥ 0xF0, e.g. 0xF1 = ecall from unmapped sepc).
    Fault {
        /// scause low byte, or a synthetic ≥0xF0 code for non-trap kills.
        cause: u8,
    },
    /// An authority terminated the task (kernel fail-fast today; policy/OOM
    /// killers later — RFC-0087).
    Killed,
}

impl ExitReason {
    /// Stable wire encoding for the high half of the `wait` status register:
    /// low byte = reason tag (0 voluntary / 2 fault / 3 killed), next byte =
    /// fault cause. Tag 1 is RESERVED (the consumer-side `error` split of
    /// voluntary exits) so tags match the ADR-0056 taxonomy 1:1.
    pub fn wire_bits(self) -> u32 {
        match self {
            Self::Voluntary => 0,
            Self::Fault { cause } => 2 | ((cause as u32) << 8),
            Self::Killed => 3,
        }
    }
}
