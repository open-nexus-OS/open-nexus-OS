<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0098: The kernel lock is split in the order the desktop in use holds it — the idle loop and the run queues, then IPC, then memory

- Status: Accepted (2026-11-11, TASK-0330 P5 — board cycle `board--2026-11-11T11-05-00`)
- Date: 2026-11-11
- Links:
  - Tasks: `tasks/TASK-0330-smp-v3-eight-harts-fdt-topology-bkl-decomposition-per-asid-shootdown.md` (P2, P3, P4), `tasks/TASK-0363-the-scheduler-s-end-form-run-queues-off-the-kernel-lock-a-fair-class-and-time-as.md` (P0: the measurement; P2, P4, P5)
  - RFCs: `docs/rfcs/RFC-0102-smp-lock-decomposition-lock-classes-ranks-and-budgets.md` (the classes and ranks this order fills)
  - Related ADRs: `docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md` (the lock today), `docs/adr/0096-a-wake-preempts-a-task-of-a-higher-class-runs-at-the-trap-exit-the-soft-realtime-chain-is-interactive.md` (preemption)
  - Measurement: `docs/board/measurements/2026-11-10-smp-baseline/README.md`

## Context

RFC-0102 names the lock classes the one kernel lock becomes; it leaves the order open on
purpose: the order follows a measurement, not a preference. Two things had to be out of the
measurement first — the capability walks (TASK-0330 P5: a receive and a close cost what they
touch) and the polling loops of services (a network owner probing for a device the board lacks,
TASK-0363 P0) — because both filled the lock with time no split would have removed.

The board then printed the baseline in ten windows of 10 s each with the desktop in use
(4 harts, pointer, keyboard, apps opened and closed):

| holder | held in 100 s | takes | per take |
|---|---|---|---|
| the idle loop | 39.2 s | 9 117 091 | 4.3 us |
| IPC (waitset wait, send, receive) | 10.6 s | 3 234 308 | 3.3 us |
| memory (`vmo_runs` almost alone) | 2.1 s | 3 555 | 594 us, up to 1.5 ms |
| scheduling syscalls | 0.10 s | 37 067 | 2.8 us |
| everything else | 0.53 s | | |

The lock is held 52.6 % of the wall time, three quarters of it by harts that have nothing to
run. Of the work's 13.4 s, IPC is 79 % and memory 16 %.

The interactive class (273 295 dispatches): 99.0 % ran within 100 us of becoming runnable,
519 waited longer than 1 ms. Of those, 508 waited behind a peer of their class on the hart they
are pinned to, 9 for a hart that was asleep, 2 behind a lower class that was not preempted. The
longest wait was 223 ms; every window with a wait above 50 ms is a window in which `vmo_runs`
held the lock for 0.4–1.5 ms per call.

## Decision

The lock is split in this order, each step a phase with its own gate:

1. **The idle loop and the run queues** (TASK-0363 P2, P4). A hart with nothing to run takes no
   shared lock: it sleeps until an interrupt, and what it reads to decide is its own run queue
   under its own lock. This removes the largest holder and the one every wake waits behind.
2. **IPC** (TASK-0330 P2): an endpoint, a waitset and a fence each under their own lock, a
   process's capability table under its own. It is four fifths of the work's lock time.
3. **Memory** (TASK-0330 P3): a lock per address space and per memory object. Before the split,
   `vmo_runs` stops walking an object under the lock (its first step): it is 16 % of the time but
   the only holder above 1 ms, and the hold tail is what a frame feels.
4. **The lock is deleted** (TASK-0330 P4), with the rank check and a budget per class.

n harts from the device tree (TASK-0330 P1) is independent of this order and comes first, as
planned: it changes how per-hart state is sized, not who holds what.

**What the split does not fix, and who does:** 98 % of the interactive class's long waits are
placement — the display chain shares one hart. No lock order removes them; the scheduler's
placement by load (TASK-0363 P5) and the display chain's time right on several harts
(TASK-0365) do. The two missed preemptions are examined before TASK-0363 P2.

## Consequences

- The plan's order (scheduler, IPC, memory) stands, now for a measured reason: the scheduler
  goes first because of the idle loop's takes, not because of the scheduling syscalls' time.
- `vmo_runs` becomes the first step of TASK-0330 P3 and gets a number: no hold above 100 us.
- Each step is judged with the same lines in the same windows (`KINIT: bkl held us`,
  `bkl top sum`, `ready latency class=interactive`), printed by the proof harness's observer
  every 10 s from `SELFTEST: input window open` on.
- A step that does not move its holder's share is wrong and is reverted, not kept.

## Alternatives considered

- **IPC first** (the largest share of the work). Rejected: IPC's wakes end in the run queue;
  with the run queues still under the one lock, an endpoint lock would be taken inside it and
  the idle loop would still stand in front of every wake.
- **Memory first** (the longest holds). Rejected as an order, taken as a step: the long hold is
  one syscall's walk, removable without splitting the address-space lock.
- **Split by subsystem size in the code.** Rejected: the capability walks were 64 % of the lock
  and a table, not a subsystem — the order follows what is measured.
