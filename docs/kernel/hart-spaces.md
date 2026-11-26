---
schema: 1
title: "The address space a hart runs on"
role: reference
subsystem: kernel
sources: [source/kernel/neuron/src/mm/hart_spaces.rs, source/kernel/neuron/src/mm/address_space.rs, source/kernel/neuron/src/core/trap/pending.rs, source/kernel/neuron/src/core/trap/fault.rs, source/kernel/neuron/src/task/reap.rs]
contracts: [ADR-0082, ADR-0100, RFC-0021]
verified: 2026-11-26
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
| who records it | the hart itself, in `hart_spaces::enter_space` — from `AddressSpaceManager::activate`, or from the dispatch that takes no lock (ADR-0100) (`HartSpaces`: one atomic entry per hart, the raw handle; `destroy` reads them under the space's lock) |
| which harts may hold a space's translations | a bit per CPU for each space, set by the hart before its `satp` names it: what a shootdown for one space reaches ([smp.md](smp.md#one-space-the-harts-that-hold-it)) |
| who writes `satp` | one function, `hart_spaces::switch_satp`, called from `enter_space` only |
| a hart left without a task | enters the kernel's space in the same lock hold |
| a free while a hart's entry names the space | refused: `AddressSpaceError::HeldByHart`, `EBUSY` at the ABI |
| a hart starting a task whose space is being freed | cannot happen: a kill or a freeze withdraws the task under the queue lock of the hart that holds it (`Scheduler::withdraw`), and a task picked there is left to that hart (ADR-0100) |

## Where a hart is left without a task

| path | code | how it leaves |
|---|---|---|
| a syscall that blocks or exits | `core/trap/handler.rs` `handle_ecall` | loads the next task's frame and activates its space; the idle task's space is the kernel's |
| a task killed by its fault | `core/trap/fault.rs` `kill_faulting_user_task` | `trap::pending::park_hart` |
| a task ended, frozen or preempted by an order, nothing else runnable | `core/trap/pending.rs` `switch_to_next` | `park_hart` |
| a syscall that entered before the kernel lock blocks its caller or requeues it | `task/wake.rs` `commit_block`, `sched/class.rs` `preempt_if_ready` | `hart_spaces::enter_kernel_space`, in the hold that gives the thread up (its thread lock, its queue's lock): before a waker, a thief or a kill can find it (ADR-0102) |
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
`KSELFTEST: address space free FAIL (pid= hart= … runs= reaper=)` — a leak that names itself
(`runs=0`: the hart is in its scheduler loop, on the space of a task it gave up) — and with it
`KINIT: space held by hart (entered=<file:line>#n left=<file:line>#n)`: where that hart last
wrote its `satp` and where it last stopped running a task, the higher count the later
(`core/smp/trail.rs`).

The kernel's own space is the machine's (`HartSpaces::name_kernel`, once at boot), never read
from PID 0's record: a record names what its task runs on and follows every attach. Until
2026-11-25 the boot selftest below attached its probe to PID 0 and detached it, the record
named no space, and every leave off the kernel lock did nothing — found by the trail line
(`entered=core/dispatch.rs left=core/trap/ipc_entry.rs`, no entry between). A hart that cannot
leave now prints `KSELFTEST: hart leave FAIL` once.

## Proof

- Host: `hart_spaces` tests, `test_reject_a_free_while_any_hart_still_runs_on_the_space`.
- Every boot: `KSELFTEST: address space held by hart ok` — a hart enters a probe space; while
  a task owns it the hart stays; with no owner left the destroy is refused, the hart leaves for
  the kernel's space, the same destroy frees it; then it enters the probe again and leaves by
  `enter_kernel_space`, the leave of the paths off the kernel lock, and PID 0's record still
  names the kernel's space (`source/kernel/neuron/src/selftest/lifetime.rs`).
- Four harts without a network device (`SMP=4`, the launcher with no netdev): the lane the
  fault showed on.
