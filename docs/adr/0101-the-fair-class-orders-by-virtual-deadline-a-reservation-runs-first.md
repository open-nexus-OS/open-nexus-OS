<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0101: The fair class orders by virtual deadline, a reservation runs first — a class is a weight, a guarantee is a time right

- Status: Accepted (2026-11-17, TASK-0363 P3)
- Date: 2026-11-17
- Links:
  - Tasks: `tasks/TASK-0363-the-scheduler-s-end-form-run-queues-off-the-kernel-lock-a-fair-class-and-time-as.md` (P3), `tasks/TASK-0365-placement-by-load-cluster-aware-the-display-chain-holds-a-time-right-and-runs-on.md` (P1: who is given which share)
  - RFCs: `docs/rfcs/RFC-0120-time-is-a-right-a-share-derived-as-a-subset-a-reservation-admitted-per-hart-revoked-in-one-step.md` (the contract this implements; its appendix carries the syscall)
  - Related ADRs: `docs/adr/0096-a-wake-preempts-a-task-of-a-higher-class-runs-at-the-trap-exit-the-soft-realtime-chain-is-interactive.md` (its class order is replaced here; its wake at the trap exit and its lines stay), `docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md` (the timer the budget end caps), `docs/adr/0100-a-queued-task-is-the-hart-s-that-takes-it-the-dispatch-takes-no-kernel-lock.md` (the queues this orders)

## Context

A hart ran four QoS classes in strict order, round-robin inside one, on a 10 ms tick; a task's
shares were extra ticks. Three things followed, measured on the board with the desktop in use:

- **A class cannot order two of its members.** Of 519 interactive dispatches later than 1 ms,
  508 waited behind a peer of the same class on the same hart
  (`build/logs/board--2026-11-13T10-38-20`). The whole display chain is `Interactive`.
- **A class is a label, not a quantity.** Whatever is raised to a class runs before everything
  below it for as long as it likes; a `Normal` task under an `Interactive` hog gets nothing.
- **Time is granted by the tick.** A task that is owed a third of a hart gets it in 10 ms
  pieces, whenever the rotation comes round.

RFC-0120 decided what a guarantee is — a reservation of a time right, admitted per hart — on a
host model. This ADR decides how one hart orders its tasks so that contract holds and
everything without a right still shares the hart sensibly.

## Decision

1. **Two lanes per hart.** A task whose reservation has budget left this period runs first,
   earliest deadline first. Everything else — and a holder whose budget is spent, until its
   deadline — is the fair class.
2. **The fair class orders by virtual deadline.** A task's virtual time grows by the time it
   ran over its weight; the hart's virtual time is the weighted mean of its tasks'. A task not
   ahead of the hart's virtual time is eligible; among the eligible, the earliest virtual
   deadline runs — its virtual time plus one slice over its weight. A tie goes to the heavier
   class, then to the one queued first.
3. **A class is a weight.** `Idle` 16, `Normal` 1024, `Interactive` 4096, `PerfBurst` 8192,
   scaled by the task's shares (1–1000, 100 the default). Two hogs share a hart in the ratio of
   their weights; nothing starves, `Idle` included.
4. **A task's lag crosses a block, a steal and a park.** It leaves a hart with the difference
   between the hart's virtual time and its own, and is placed by it where it arrives, bounded
   by one slice either way: a sleep earns no more than one turn, a long run costs no more.
5. **A wake goes ahead when the order says so.** A reservation with budget goes ahead of a fair
   task and of a later deadline. Between fair tasks, an earlier virtual deadline goes ahead if
   it is heavier, or once the running task has run past the hart's virtual time — an equal
   wake waits until the running task had its share. The order is asked when a task came into
   it (a wake, a park, a queued task's new class or reservation — the queue's arrival mark) and
   at the running task's timer, not at every trap's exit: asked at every syscall's end, two
   peers that make syscalls trade the hart at each one, and every exit pays the meter. The
   question and the switch it leads to are one queue operation (`Scheduler::switch_if_ready`).
6. **A yield goes behind every task queued**; a tick or a wake ahead of a task leaves its
   virtual time as it is.
7. **Reserved time counts against the fair share.** A holder gets the larger of its budget and
   its fair share, not their sum: a reservation buys *when* a task runs.
8. **The order is asked at every timer interrupt of a user task**, and a budget is enforced to
   the clock: the hart's timer is capped at the running holder's budget end. The rotation of
   shares as extra ticks is deleted.
9. **A holder runs on the hart that admitted its reservation.** Its wake goes there and it is
   never stolen. Moving a reservation by load (the contract's `migrate`) is TASK-0365's.
10. **A hart is reserved to four fifths at most**; the fair class keeps the rest of every hart.
    The kernel holds 64 rights and 64 reservations.
11. **What a guarantee is judged by is counted**: a budget that ran out under its holder (an
    overrun — it runs on fair), and a period in which a runnable holder did not get its budget
    (a miss). A holder's waits are measured in a lane of their own.
12. **No marker states a guarantee** while kernel paths serialize behind the kernel lock
    (RFC-0120, G1). The mechanism runs; its lines are measurements.

## Consequences

- The display chain's place in the order is a reservation init binds (`init: time rights`),
  no longer the class alone; the class stays as its weight past its budget.
- A pick scans the hart's queued tasks (at most 64 per class) under the hart's own lock, with no
  allocation. The queues stay a leaf lock; the time-right table is its own lock, taken before a
  hart's queues and never the other way.
- A task's run time is metered at every switch (`sched/meter.rs`); the meter feeds the virtual
  time, the budget and the table's books.
- ADR-0096's rule "a wake of a higher class preempts" is replaced by rule 5. Its attribution of
  late waits (`late_idle`, `late_lower`, `late_peer`) still reads classes.

## Alternatives considered

- **Strict classes, deadlines only for holders.** Keeps the starvation and the peer ordering
  for everything without a right. Rejected.
- **Weighted round-robin on the tick** (the shares as extra ticks). No lag, 10 ms granularity,
  and a wake's latency does not depend on what the woken task is owed. Rejected.
- **Least virtual time first, without eligibility.** A task back from a long sleep is far
  behind and runs until it caught up, unless placed by a rule of thumb. The virtual deadline
  with eligibility bounds lag by one slice with no heuristic. Rejected.
- **Reserved time on top of the fair share.** A holder would get its budget and then its full
  fair share: more than it was given. Rejected.
- **A holder stolen like a fair task.** Its budget and deadline would run on a hart that did
  not admit them. Rejected until the move is an admitted one (TASK-0365).

## Proof

```bash
RUSTFLAGS='--cfg nexus_env="host"' cargo test -p neuron --lib run_queue
cargo test -p nexus-sched --test time_right
```

- Host: `run_queue_order_tests.rs` — the weights' ratio within 5 %, the budget first every
  period for a hundred periods with each overrun counted, a miss counted and not miscounted,
  the lanes, the wake rule, the lag across harts, every `test_reject_*`.
- Kernel: `KSELFTEST: sched fair ok`, `KSELFTEST: sched time right ok`,
  `KSELFTEST: sched runtime metered ok`, and the syscall's `KSELFTEST: test_reject_time_right_*`
  (multi-hart lanes and the board).
- Board: `KINIT: ready latency class=reserved` (the holders' waits, `le200us`), `KINIT: time
  rights (… overruns= misses=)`. Measured (`build/logs/board--2026-11-18T08-25-01`, 8 harts, the
  desktop in use): 99.26 % of 163 507 holder wakes within 200 us, none above 10 ms, 4 periods
  missed in the session; interactive waits above 10 ms 12 (166 under strict classes).
- Not explained: one cycle (`board--2026-11-17T21-09-19`) held the kernel lock four times for
  144–369 ms and a holder's wake waited 199 ms; no other cycle shows it. A guarantee is not
  stated before the kernel lock is gone (RFC-0120 G1); `KINIT: bkl stall` names the next one.
