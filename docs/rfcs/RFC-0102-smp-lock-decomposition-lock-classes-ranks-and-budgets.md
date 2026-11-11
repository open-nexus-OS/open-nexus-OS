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

The order in which the lock is taken apart is **not decided here**: it is ADR-0098
(`docs/adr/0098-the-kernel-lock-is-split-in-the-order-the-desktop-in-use-holds-it.md`), written
from the board's windows with the desktop in use (`KINIT: bkl held us`, the lock's time per
subsystem) — the idle loop and the run queues, then IPC, then memory.

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

### A held capability is counted at its object

The counts are not kept by the paths that install or drop a capability — there are sixty of
them — but by the type that is held (TASK-0330 P5, `cap/held.rs`):

- An endpoint and a memory object each own a **header** (`HeldCounts`): the object's identity,
  an endpoint's owner, and four atomic counts — the capabilities held to it (`any`), those with
  the send right, those with the receive right, and the send ones that sit in the owner's own
  table.
- A **held capability** (`Held`) is one capability someone holds: its descriptor (kind,
  rights) and a reference to the header. It is neither copied nor cloned. Its drop counts it
  off.
- A table slot holds a `Held`, and so does a message that moves one. A move changes no count:
  it is the same held capability in the sender's slot, in the queue and in the receiver's
  slot, so no observer ever sees a count lower than the truth.
- A table knows its holder. Installing a send capability of an endpoint into its owner's
  table, or taking it out, is the one place the fourth count changes: the owner keeps the send
  base of its own reply channel and is never its own peer.
- A descriptor (kind and rights without the reference) stays a plain value: what a rights
  check, a query or a log line reads. It holds nothing and installs nowhere — every door into
  a table and into a message takes a `Held`.

**What a held capability is never.** Its kind and its rights are fixed when it comes into
being; the fields are private to one file and nothing writes them. So the object a capability
names and what it may do with it cannot be changed by whoever holds it, by a message that
carries it, or by a kernel path that handles it — the receive path used to rewrite the
endpoint a moved capability named from a second field of the message; that field and the
rewrite are deleted. A held capability comes into being through five doors, none of them
widening:

| Door | Makes | Refuses |
|---|---|---|
| `mint` at the header (the router for a live endpoint, the object table for a live memory object) | the object's capability with the rights the kernel path states | a dead endpoint, an object that is gone — there is no header to mint at |
| `mint_read_only` at a memory object's header | a read-only capability to an object nobody holds writably (the device tree, the console ring, the boot image's files) | an endpoint |
| `dup` of a held capability | the same kind at the same object, a subset of its rights | any right the source lacks |
| `read_only_alias` of a held writable memory-object capability | the read-only kind at the same object, a subset of its rights | every other kind — a read-only alias has no alias, and no door leads back to the writable kind |
| `plain` from a descriptor | a kind that carries no counts (a timer, a waitset, a fence, a device window, an interrupt, a factory, a scheduling right) | an endpoint or a memory object: a counted kind is never made from a number |

Each refusal has its `test_reject_*` on the host (`cap/tests_prop.rs`), beside the model test
that holds the counts against a census over 10 000 random operations.

A memory object is not destroyed while a capability is held to it (`VmoError::Held`): the
object's id goes back to the table only at `any` zero, so a held capability never names the
next object that gets the id. A dead endpoint's id is handed out again only at `any` zero for
the same reason.

What the paths read: a foreign sender remains while `send` exceeds the owner's own; a receiver
remains while `recv` is above zero; an endpoint dies at `any` zero; a memory object is
reclaimed at `any` zero once nothing maps it, and destroyed by its sole owner at `any` one. A
path that dropped a capability judges after the drop. The counts are atomics (rank 8, no
lock): a capability is dropped under its process's lock, the judgement that follows takes the
object's lock — ascending.

The walks over the address spaces that ask who maps a memory object (the reclaim's, the
seal's, the squeeze's) are the same sin on the other table; a memory object counts its
mappings when the address spaces leave the lock (TASK-0330 P3). Until then the reclaim's runs
only when the last capability to an object has gone; the seal's and the squeeze's stay at
their own syscalls.

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
