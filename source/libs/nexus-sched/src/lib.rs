// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
//! CONTEXT: The scheduling vocabulary the kernel and userspace share — `CpuSet`, the set of
//! logical CPUs a task may run on, and the topology a program asks the kernel for (RFC-0118;
//! one definition on both sides of the ABI) — and a deadline helper for userspace services.
//! OWNERS: @kernel-team @runtime
//! PUBLIC API: CpuSet, CpuSetError, Topology, TopologyQuery, TOPOLOGY_OP, Placement, Deadline
//! DEPENDS_ON: core
//! INVARIANTS: Pure calculations, no allocation, no unsafe; unit-tested
//! ADR: docs/adr/0016-kernel-libs-architecture.md

#![forbid(unsafe_code)]

mod cpu_set;
mod topology;

pub use cpu_set::{CpuSet, CpuSetError};
pub use topology::{Placement, Topology, TopologyQuery, TOPOLOGY_OP};

/// Simple scheduler timeline helper for user space services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline {
    pub ticks: u64,
}

impl Deadline {
    pub const fn from_ms(ms: u64) -> Self {
        Self { ticks: ms * 1_000 }
    }

    pub fn expired(self, now: u64) -> bool {
        now >= self.ticks
    }
}

#[cfg(test)]
mod tests {
    use super::Deadline;

    #[test]
    fn expiry_checks() {
        let deadline = Deadline::from_ms(10);
        assert!(!deadline.expired(5_000));
        assert!(deadline.expired(10_000));
    }
}
