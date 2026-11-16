<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0100: A queued task is the hart's that takes it — its frame lives in a record the dispatch reads without the kernel lock

- Status: Accepted (2026-11-16, TASK-0363 P2 — board `board--2026-11-16T12-40-08`: no breach of the owner rule, the idle loop's dispatch holds the kernel lock 0 us, the lock's longest wait 399 us)
- Date: 2026-11-16
- Links:
  - Tasks: `tasks/TASK-0363-the-scheduler-s-end-form-run-queues-off-the-kernel-lock-a-fair-class-and-time-as.md` (P2: S1, S2, S3)
  - RFCs: `docs/rfcs/RFC-0102-smp-lock-decomposition-lock-classes-ranks-and-budgets.md` (the classes)
  - Related ADRs: `docs/adr/0098-the-kernel-lock-is-split-in-the-order-the-desktop-in-use-holds-it.md` (the order: the idle loop and the run queues first), `docs/adr/0082-a-process-is-its-address-space-the-parent-orders-it-whole-freeze-thaw-kill-and-a-frozen-receiver-refuses-the-park.md` (pending orders)

## Context

ADR-0098 put the idle loop first: a hart with nothing to run held the kernel lock 39 s of 100 s
on the board, to look at its own queue. S1 gave each hart's queues their own lock, but the
dispatch still read the task's frame, its floating-point states and its address space out of
the task table and the address-space manager — both the kernel lock's — and the table moves when
it grows. A dispatch without the lock needs those at a fixed address, a rule for who may touch
them, and a kill or a freeze that cannot reap a task a hart is starting.

## Decision

- **A task's frame and floating-point states live in its scheduling record** (`sched/record.rs`),
  at a fixed address for the kernel's life; the task table only points at it. The record also
  carries the task's space as its dispatch switches to it (`satp`, cache-maintenance right),
  written by the address-space manager where it records the task as the space's owner.
- **The record has one holder** (`run_queue::Owner`): free (blocked, new, exited), queued on a
  hart, or running on a hart. The scheduler hands it on under the queue lock of the hart
  concerned at every enqueue, pick, steal, block, finish and purge. A frame is written before
  its task is queued and read after it is picked — by nobody in between. Every access checks the
  rule and names a breach (`KINIT: FAIL frame owner … at=file:line`).
- **A kill and a freeze withdraw** (`Scheduler::withdraw`): under the queue lock of the hart that
  holds the task they either take it off its queue or find it running there and leave a pending
  order. A task a hart has picked is never reaped under it.
- **The idle loop picks from its own queue without the kernel lock** and starts the task from
  its record. A task the record cannot start (no space recorded, not a user frame) goes to the
  locked path already picked; the steal and the idle hart's deliveries (timers, interrupts,
  deadlines) keep the lock until their own steps.
- Out of scope here: the timer preemption and the syscall exit's switch (they run inside a trap
  that holds the lock for other reasons), the steal, and the deadline walk (TASK-0363 P6).

## Measured

Board, ten windows of 10 s (`KINIT: bkl idle split`): the idle loop's dispatch 0 us and 0 takes;
the secondaries' steal 0.03-0.14 s; the boot hart's delivery 3.4-4.7 s (timers, interrupts and
pressure 1.6-2.2 s, the deadline walk 1.4-2.0 s) — the idle group's total is that delivery now,
which TASK-0363 P6 (one deadline walk) and P4 (the boot hart sleeps until an event) take off.
The kernel lock's longest wait fell from 5-6 ms (before S2) to 399 us.

## Consequences

- **Positive**: the largest holder of the kernel lock is no longer needed to start a task; the
  lock rule a dispatch relies on is checked on every frame access, release builds included; two
  orderings that would have handed a thief a stale frame were found by it before the lock went
  (the preemption saved after its requeue, `yield` requeued before its return value).
- **Negative**: every frame access pays an owner check (about 3 us of an IPC round trip in QEMU);
  the record duplicates nothing, but the space view is a projection the manager must keep (its
  `attach`, `detach` and the cache grant are the only writers).
- **Churn accepted**: `Task::frame()` keeps its signature; `TaskTable` no longer holds the current
  task per hart (`sched::runs_here`); `HartSpaces` entries are atomic.

## Alternatives considered

- **A lock per task.** Rejected: the queue lock already serialises who holds a task; a second lock
  per task adds an order to keep and a take on every dispatch.
- **Copying the frame into the run-queue entry.** Rejected: a frame is 272 bytes, an entry 24; a
  steal would copy it twice, and the syscall paths that write a free task's frame would need the
  queue's lock.
