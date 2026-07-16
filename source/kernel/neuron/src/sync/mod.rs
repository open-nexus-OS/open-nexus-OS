// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel synchronization primitives for SMP (TASK-0283/TASK-0277)
//! OWNERS: @kernel-team
//! STATUS: In Progress
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests via cargo test -p neuron (percpu, spin_irq)
//! PUBLIC API: PerCpu, SpinIrqLock, dbg_mutex (debug builds)
//! DEPENDS_ON: spin::Mutex, sstatus CSR (OS target only)
//! INVARIANTS: SpinIrqLock is the only lock type permitted in trap-reachable
//!             paths (lock acquisition implies SIE off for the hold duration);
//!             PerCpu slots are only reachable for the executing CPU.
//! ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md
//!
//! Lock hierarchy (normative, TASK-0277): KernelShared (BKL) -> per-CPU run
//! queue locks (ascending CPU index) -> leaf atomics. Never acquire the BKL
//! while holding a run-queue lock.

pub mod percpu;
pub mod spin_irq;

#[cfg(all(target_os = "none", debug_assertions))]
pub mod dbg_mutex;
