---
schema: 1
title: "The address space a hart runs on"
role: reference
subsystem: kernel
sources: [source/kernel/neuron/src/mm/hart_spaces.rs, source/kernel/neuron/src/mm/address_space.rs, source/kernel/neuron/src/core/trap/pending.rs, source/kernel/neuron/src/core/trap/fault.rs, source/kernel/neuron/src/task/reap.rs]
contracts: [ADR-0082, RFC-0021]
verified: 2026-11-12
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The address space a hart runs on

> **Purpose** — which page table each hart's `satp` names, when a hart changes it, and why a space is never freed under a hart.
> **Owns** — `source/kernel/neuron/src/mm/hart_spaces.rs`
> **Contracts** — [ADR-0082](../adr/0082-a-process-is-its-address-space-the-parent-orders-it-whole-freeze-thaw-kill-and-a-frozen-receiver-refuses-the-park.md), [RFC-0021](../rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md)
> **Invariants** — a hart's `satp` names the space of the task current on it, or the kernel's own. A space a hart runs on is never freed.
> **Read next** — [smp.md](smp.md), [README.md](README.md)

## The rule

| | |
|---|---|
| who records it | the hart itself, in `AddressSpaceManager::activate`, under the lock that also covers `destroy` (`HartSpaces`, one entry per hart, the raw handle) |
| who writes `satp` | one function, `hart_spaces::switch_satp`, called from `activate` only |
| a hart left without a task | enters the kernel's space in the same lock hold |
| a free while a hart's entry names the space | refused: `AddressSpaceError::HeldByHart`, `EBUSY` at the ABI |

## Where a hart is left without a task

| path | code | how it leaves |
|---|---|---|
| a syscall that blocks or exits | `core/trap/handler.rs` `handle_ecall` | loads the next task's frame and activates its space; the idle task's space is the kernel's |
| a task killed by its fault | `core/trap/fault.rs` `kill_faulting_user_task` | `trap::pending::park_hart` |
| a task ended, frozen or preempted by an order, nothing else runnable | `core/trap/pending.rs` `switch_to_next` | `park_hart` |
| a task released from its own exit (a thread that outlived its leader) | `task/reap.rs` `release_reaped` | `AddressSpaceManager::leave` before the release — only when no task owns the space any more: a process reaping one of its threads runs on the space it shares with it and stays |

## Why it is a rule

A page table's frames return to the pool at the free. A hart whose `satp` still names them
walks whatever they hold at its next translation miss: a kernel page fault at best, a stray
write at worst. Until 2026-11-11 the two trap-side paths left the hart on the dead task's
space, and the parent's reap on another hart freed it. One service polling by `yield` gave
every idle hart work within a millisecond and hid the window; the first boots without it (the
board, no network plane) froze in four of five
(`docs/board/measurements/2026-11-10-smp-baseline/README.md`).

A refused free in a running system keeps the page table and prints
`KSELFTEST: address space free FAIL (pid= hart=)` — a leak that names itself.

## Proof

- Host: `hart_spaces` tests, `test_reject_a_free_while_any_hart_still_runs_on_the_space`.
- Every boot: `KSELFTEST: address space held by hart ok` — a hart enters a probe space; while
  a task owns it the hart stays; with no owner left the destroy is refused, the hart leaves for
  the kernel's space, the same destroy frees it (`source/kernel/neuron/src/selftest/lifetime.rs`).
- Four harts without a network device (`SMP=4`, the launcher with no netdev): the lane the
  fault showed on.
