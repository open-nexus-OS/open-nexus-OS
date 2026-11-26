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
- **The steal takes no kernel lock either** (2026-11-25, TASK-0330 P4). The thief holds its own
  queues and tries its victim's: a taken task arrives as the thief's running task in the hold
  that takes it from its queue, so it is on one hart at every look and a kill or a freeze needs
  no lock above the queues' to find it (a walk over the harts can pass it — `Absent` for a
  held record means look again, `TaskTable::park`). Affinity and home are read from the
  record; the thief becomes its home and its device lines follow, the home read in the hold
  that moves them (`irq::rehome`). The stolen task is dispatched as a picked one is.
- **A `yield` takes no kernel lock** (2026-11-25, TASK-0330 P4): counted, then the caller is
  queued behind every task waiting on its hart under the queue lock alone, and the hart's loop
  picks; with nothing waiting it returns at once. A deadline that is due sends it to the locked
  path, whose walk delivers it — a task that polls by `yield` still wakes the tasks waiting
  beside it. QEMU: the scheduling syscalls' holds 1.8 s -> 0.07-0.11 s a window.
- **A due timer cap takes no kernel lock** (2026-11-25, TASK-0330 P4): the timer table is
  behind its own lock, and a secondary with nothing to run and only timer caps due delivers
  them itself (`idle::caps_due_alone`, `trap::deliver_timers`), then looks at its queue again.
  A delivery is one hold of the table, from the pop to the last event sent: a timer's events
  reach its endpoint in order whichever harts deliver, and an endpoint's death comes after
  every fire popped for it. A blocked task's deadline, a stashed line and the boot hart's
  delivery still take the lock. QEMU: the idle harts' holds 0.76 s -> 0.07 s a window on four
  harts, 1.8-2.2 s -> 0.12-0.14 s on eight (takes 8 600-11 000 -> about 1 000).
- **A dispatch never pushes a due deadline out** (2026-11-26, TASK-0330 P4). Every task started
  from the scheduler loop rewrote the preemption tick, and the arm left a deadline that was
  already due out. Since IPC blocks off the kernel lock every switch passes through the loop:
  one hart switching between two tasks faster than the tick took no timer interrupt — no timer
  cap and no deadline for 6 s (headless). The dispatch arm reads the hart's own floor, arms "now"
  for a due deadline and keeps interrupts masked until the user code runs, where the handler
  delivers it; an idle hart that delivers on several harts had hidden it.
- Out of scope here: the timer preemption and the syscall exit's switch (they run inside a trap
  that holds the lock for other reasons) and the deadline walk (TASK-0363 P6).

## Measured

Board, ten windows of 10 s (`KINIT: bkl idle split`): the idle loop's dispatch 0 us and 0 takes;
the secondaries' steal 0.03-0.14 s; the boot hart's delivery 3.4-4.7 s (timers, interrupts and
pressure 1.6-2.2 s, the deadline walk 1.4-2.0 s) — the idle group's total is that delivery now,
which TASK-0363 P6 (one deadline walk) and P4 (the boot hart sleeps until an event) take off.
The kernel lock's longest wait fell from 5-6 ms (before S2) to 399 us.

The steal off the lock, QEMU, eight harts, one window of 22-25 s (2026-11-25): the secondaries'
idle holds 4.12 s in 26 272 takes before, 1.4-2.0 s in 9 100-11 000 after — what is left is
their due timer deliveries (`due=` in `KINIT: bkl idle split`); four harts 0.88 s -> 0.60 s.

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
