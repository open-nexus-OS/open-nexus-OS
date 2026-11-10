---
schema: 1
id: RFC-0102
title: "SMP lock decomposition: no global kernel lock — lock classes, one rank order, a budget per class"
status: draft
owners: ["@kernel-team"]
created: 2026-11-10
subsystem: kernel
tasks: [TASK-0330, TASK-0363, TASK-0364]
depends_on: [RFC-0021, RFC-0096, RFC-0118]
links:
  - label: "Today's lock classes"
    target: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md
  - label: "Object lifetime"
    target: docs/adr/0073-an-object-dies-with-its-last-capability-a-death-is-an-event-on-an-endpoint.md
  - label: "Time as a right (gate G1)"
    target: tasks/TRACK-TIME-AS-RESOURCE.md
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0102: SMP lock decomposition: no global kernel lock — lock classes, one rank order, a budget per class

## Why

Every mutable kernel structure — run queues, tasks, the IPC router, address spaces, timers,
waitsets, fences — is reached through one lock (`KernelGuard`). ADR-0049 bounded it by policy
(phased syscalls, a lock-free class, right of way for CPU 0) and the display chain was pinned to
one hart so it would not queue behind the others. With eight harts the pin is the cost: seven
services share one hart while three to seven idle. A bound that is a measurement is also not a
property (TRACK-TIME-AS-RESOURCE, gate G1). The end state is no global lock.

The order in which the lock is taken apart is **not decided here**: it is written as an ADR from
TASK-0330 P0's numbers (`KINIT: bkl held us`, the lock's time per subsystem), as the task's red
flag demands.

## Scope

- **Owns:** the lock classes, their rank order, what may be held across what, the debug rank
  check, the budget per class and its gate.
- **Does not own:** the CPU set and per-hart state (RFC-0118), the scheduler's policy and time
  rights (TASK-0363's RFC), threads and the wait-on-a-word ABI (TASK-0364's RFC).

## Contract (normative)

### Classes and ranks

A hart takes locks in ascending rank only. Two locks of one class are taken in the order the
class states, or never together.

| Rank | Class | Guards | Two at once |
|---|---|---|---|
| 1 | registry | a table from an id to its object (tasks, endpoints, memory objects, address spaces) | never; held for the lookup only |
| 2 | process | a process's capability table, thread list and lifecycle state | parent before child |
| 3 | address space | a space's regions and page-table edits | never |
| 4 | memory object | an object's pages, commit state and seals | never |
| 5 | IPC object | an endpoint's queues and waiters; a waitset; a fence; a timer | by object address |
| 6 | thread | a thread's run state, block reason and wait links | never |
| 7 | run queue | one hart's queues and its current task | ascending CPU index |
| 8 | leaf | a frame magazine or bank, the kernel heap, the console ring | never; nothing is taken under a leaf |

- A **registry** lock is released before the object's own lock is taken: the lookup returns a
  counted reference (ADR-0073's lifetime), so an object cannot die under its user.
- The **wake** path is why threads and run queues rank below the IPC objects: a send holds the
  endpoint, marks the receiver runnable (thread), and enqueues it (run queue).
- The **fault** path takes the address space, then the object, then a leaf for the frame.
- Per-hart data that only its own hart touches is not locked (`PerCpu`); counters are atomics.
- **No path walks another process's table.** What a path must know about an object's holders,
  the object carries: an endpoint counts the capabilities that name it (send, receive, any)
  and a memory object its names, updated where a capability is installed, moved into a
  message, received or dropped; the last-capability rule of ADR-0073, the last-sender end of a
  queue and the reclaim of an unnamed memory object read the counts. The walk over every table
  that a blocking receive and a capability close ran under the global lock — the two largest
  holders of TASK-0330 P0's first measurement — is deleted before the IPC objects leave the
  lock (TASK-0330 P5): under the rank order it could not be written at all.

### What a lock may be held across

- Every lock is a `SpinIrqLock`: interrupts are off on the hart while it is held. No lock is
  held across a context switch, a return to user mode, a wait for another hart, or a console
  write that can wait.
- A hart spinning for a lock holds none below rank 8 that a shootdown responder needs, and keeps
  answering shootdown requests (the rule the BKL's spin already follows).
- Work whose length a caller decides (a copy, a zero fill, a range of pages) is staged under the
  lock and done outside it (ADR-0049's phased class becomes the general rule), or bounded per
  take by a constant the budget covers.

### The rank check

Debug builds keep the highest rank a hart holds in its `HartLocal`; taking a rank at or below it
(outside the stated two-at-once orders) panics with both lock names. The check is the lock
type's, not the caller's, so no take can skip it. Its logic is host-tested.

### Budgets

Each class has a longest hold and a longest wait in microseconds (`core/trap/budgets.rs`, the one
table), set from the board's measurement when the class leaves the global lock and judged at the
boot-end fence: `KSELFTEST: lock budgets ok (class=max_hold/max_wait …)`; a breach is the FAIL
form and fails the lane. The global lock's budget line (`KSELFTEST: bkl budget ok`) is deleted
with the lock, in the same change as its entries in `qemu-test.sh`, the marker lists and the docs.

### The end

`KERNEL_LOCK`, `KernelGuard` and the right of way for CPU 0 are deleted; their names are retired.
The lock-free syscall class and the phased class of ADR-0049 remain as what they are: a syscall
that takes no lock, and work done outside the lock.

## Invariants

| Invariant | Proof |
|---|---|
| a lower rank is never taken under a higher one | `test_reject_lock_rank_inversion` (host) + the debug check on every QEMU lane |
| two locks of a class only in the class's order | `test_reject_two_run_queues_descending`, `test_reject_two_endpoints_out_of_order` |
| nothing is taken under a leaf | `test_reject_lock_under_a_leaf` |
| no lock across a switch or a return to user mode | the debug check at the switch and at `sret`: held rank must be none |
| an object outlives its users | the lifetime selftests of ADR-0073 kept green at 8 harts |
| an endpoint's and a memory object's counts equal the capabilities naming them | host model: the counts against a census after 10 000 random capability operations; retired-name gate for the census walk |
| a hold stays within its class's budget | `KSELFTEST: lock budgets ok` on the board at 8 harts |

## Failure model

A rank violation or a lock held at a switch is a kernel bug: debug builds panic with the names,
release builds have no check and no cost. A budget breach fails the gate, not the boot. A lost
shootdown acknowledgement stays a panic (RFC-0021).

## Security

Identity, capability checks and policy calls do not move: a check that ran under the global lock
runs under the lock of the object it reads. A capability table belongs to its process and is
never read under another process's lock. No check becomes a check-then-use across a lock release:
where a path drops and retakes a lock (the phased class), it re-validates what it kept.

## Proof

```bash
just test-kernel            # rank check, class orders
just ci-os-smp1 && just ci-os-smp
just ci-os-smp8             # the stress lane (new with TASK-0330 P1), part of test-all at P4
```

Board: `KSELFTEST: lock budgets ok` at 8 harts; `KSELFTEST: ipc call budget ok` on every hart.

## Alternatives considered

- **Keep the global lock, shorten its holds.** Rejected: that is ADR-0049, and its bound is a
  measurement; eight harts queue behind one syscall however short.
- **A kernel per hart (a multikernel).** Rejected in TASK-0330: every capability, task and
  memory object twice on a coherent eight-hart SoC, for a scaling the lock decomposition gives.
- **Read-copy-update for the registries.** Not now: a short registry lock with counted
  references is simpler to prove; if the registry class shows in the budgets, that is its ADR.
- **A preemptible kernel with sleeping locks.** Rejected: holds are bounded and short by this
  contract; sleeping locks add priority inversion inside the kernel for nothing.
