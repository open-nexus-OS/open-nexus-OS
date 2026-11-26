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
//! Lock hierarchy (normative, RFC-0102): the kernel lock (until TASK-0330 P4) -> the ranked
//! classes in ascending rank (`lock_rank::Class`; `ranked::RankedLock` asks the rule on every
//! take) -> leaf atomics. Never acquire the kernel lock while holding a ranked lock.

pub mod block_table;
pub mod class_stats;
pub mod hart_table;
pub mod loan_word;
pub mod lock_rank;
pub mod noted_bound;
pub mod percpu;
pub mod ranked;
pub mod slot_table;
pub mod spin_irq;
pub mod wait_word;

#[cfg(all(target_os = "none", debug_assertions))]
pub mod dbg_mutex;
