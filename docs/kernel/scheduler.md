---
schema: 1
title: "The scheduler"
role: reference
subsystem: kernel
sources: [source/kernel/neuron/src/sched, source/kernel/neuron/src/task/wake.rs, source/kernel/neuron/src/core/trap/baseline.rs, source/libs/nexus-sched/src/time_right.rs]
contracts: [RFC-0120, RFC-0021, ADR-0052, ADR-0096, ADR-0101]
verified: 2026-11-17
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The scheduler

> **Purpose** — which task a hart runs, where a woken task goes, and how the time a task ran is metered.
> **Owns** — `source/kernel/neuron/src/sched/`, the placement in `task/wake.rs`
> **Contracts** — [RFC-0120](../rfcs/RFC-0120-time-is-a-right-a-share-derived-as-a-subset-a-reservation-admitted-per-hart-revoked-in-one-step.md), [RFC-0021](../rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md), [ADR-0052](../adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md), [ADR-0096](../adr/0096-a-wake-preempts-a-task-of-a-higher-class-runs-at-the-trap-exit-the-soft-realtime-chain-is-interactive.md)
> **Invariants** — a hart's queues sit behind its own lock, a leaf. A queue operation never allocates and a full queue rejects deterministically. A task is never lost or doubled by a steal. A queued task's frame is touched by nobody. Every tick a hart ran a task is charged to that task once. A reservation with budget runs before everything fair; a hart is never reserved past its capacity; no time is minted.
> **Read next** — [smp.md](smp.md), [idle.md](idle.md), [topology.md](topology.md)

## Run queues

Every hart has its own run queues and its own lock (`sched::RUN_QUEUES`, TASK-0363 P2): a wake,
a steal and a switch on one hart never wait for a syscall on another. The rules are a pure core
(`sched/run_queue.rs`, host-tested in `run_queue_tests.rs`); `sched/mod.rs` names the executing
hart, takes the lock and stamps the clock.

Storage: one bounded queue per QoS class and hart (64 tasks each) and the task the hart runs.

## Order

Two lanes per hart (`sched/run_queue_order.rs`, ADR-0101), host-tested in
`run_queue_order_tests.rs`:

1. **A reservation with budget left runs first**, earliest deadline first.
2. **Everything else is fair by weight** — and a holder whose budget is spent, until its
   deadline. A task's virtual time grows by the time it ran over its weight; the hart's virtual
   time is the weighted mean. Among the tasks not ahead of it, the earliest virtual deadline
   runs (virtual time plus one slice over the weight).

| Class | `Idle` | `Normal` | `Interactive` | `PerfBurst` |
|---|---|---|---|---|
| Weight at 100 shares | 16 | 1024 | 4096 | 8192 |

A class is a weight, not a priority: two hogs share a hart in the ratio of their weights (times
their shares, 1–1000), and nothing starves. Rules that follow:

- A wake goes ahead of the running task at the trap exit when it holds a reservation with budget
  (before a fair task or a later deadline), or — fair against fair — when its virtual deadline
  is earlier and it is heavier, or the running task has run past the hart's virtual time. The
  order is asked only when a task came into it (the queue's arrival mark) and at the timer.
- A task's lag crosses a block, a steal and a park, bounded by one slice either way: a sleep
  earns one turn at most.
- A `yield` goes behind every queued task; a tick or a preempting wake keeps the virtual time.
- Reserved time counts against the fair share: a holder gets the larger of the two.
- Every timer interrupt of a user task asks the order again.

Markers: `KSELFTEST: sched fair ok (weights=4:1 heavy_us= light_us= off_pm= max_hold_us=)` —
two hogs driven on the boot hart for 100 ms are metered within two of the longest holds of their
shares (within 5 % when the host stalled none).

## Time rights

A guarantee is a reservation of a time right (RFC-0120): a share of one hart every period,
admitted against the hart's capacity (80 %; the fair class keeps the rest).

| Piece | Where | What it holds |
|---|---|---|
| The contract | `nexus_sched::TimeRights` | the tree of rights, the reservations, admission, revoke by generation — host-tested |
| The kernel's table | `sched/reserve.rs` | 64 rights, 64 reservations, behind its own lock (taken before a hart's queues, never after) |
| A task's reservation | its record (`Record::reservation`) and a copy in its queue entry (`run_queue::Res`) | the id; this period's deadline and what is left of the budget |
| The syscall | `syscall/api/time_right.rs`, sched syscall operation 8 | claim the root (init, once), held, derive, bind, unbind, revoke — authority is the `TimeRight` capability |

- **A bind** admits the reservation on the task's home hart, else on another its affinity
  allows; a holder runs on that hart (its wake goes there, it is not stolen).
- **A wake** applies the constant-bandwidth rule: a sleep brings back no time.
- **The budget end** caps the hart's timer (`trap::timer_arm`): a budget is enforced to the
  clock. Past it the holder runs fair; the overrun is counted.
- **A miss** is a period in which a runnable holder did not get its budget — counted in the
  queues (`HartQueues::take_missed`).
- **init** claims the root, derives the display chain's right and binds the renderer, the
  compositor and the input router before their wake (`nexus-init/src/affinity.rs`
  `DISPLAY_TIME`); line `init: time rights (root_ppm= delegated_ppm= bound_ppm= bound= fail=)`.

Markers: `KSELFTEST: sched time right ok (budgets= of_us= ran_us= periods= missed= stalled=)` —
a holder of a fifth of the boot hart every 2 ms, against a hog of eight times its weight, runs a
hundred budgets in full, misses no period (but those a measured stall of the host covered), and
the books balance after unbind and revoke. `KSELFTEST: test_reject_time_right_claim_by_stranger
ok`, `…_forged_slot ok`, `…_beyond_held ok`, `…_bind_other_task ok`, `…_after_revoke ok` — the
syscall's refusals. No marker states a guarantee while kernel paths wait for the kernel lock
(RFC-0120, G1).

## Records

A task's frame, floating-point state and address space live in its scheduling record
(`sched/record.rs`, fixed address): the hart whose queue holds it, or the hart that runs it, owns
it (`run_queue::Owner`); a breach prints `KINIT: FAIL frame owner`. The dispatch takes no kernel
lock.

## Placement and steal

A wake places its task where it runs soonest (`sched/placement.rs`, TASK-0363 P5): at home when
home takes it at once (asleep, the waker's own, or running a lower class) and nothing waits
there; else on a free hart its affinity allows, home's cluster first — that hart becomes its
home. A hart chosen a moment ago is not free until it looks (its wake hint), so a burst of wakes
spreads. Marker: `KSELFTEST: sched spread ok`.

A steal (`run_queue::steal`) takes at most one task per look, never above the thief's class nor
against affinity; a task the thief may not run is parked at the front of its home hart's queue
(ADR-0052). Markers: `KSELFTEST: work stealing ok`, `steal park ok`,
`test_reject_steal_above_bound ok`, `test_reject_steal_higher_qos ok`.

## The meter

Every operation that changes the task a hart runs — a pick, a yield, a block, an exit, a steal,
a purge — is metered (`sched/meter.rs`, the rule `HartQueues::charge`): the running task is
charged the time since its last charge, into its record (`Record::runtime_ns`) and the hart's
window sum. A task's run time includes the kernel's work on its behalf (its traps and syscalls).
This is the meter RFC-0120 names (the track's gate G2): the fair class's virtual time, a
reservation's budget and the table's books are fed from it.

Marker `KSELFTEST: sched runtime metered ok (ran_us= of= span_us=)`: a task held on the boot
hart for 2 ms through the scheduler's switch path is charged at least 2 ms and at most the span
from before its pick to after its switch away, on the same clock. The window line
`KINIT: hart busy pm (… mean= metered= placed=)` prints the harts' mean busy share and the part
of it charged to tasks; the rest is the idle loop awake (its walks, steals and deliveries).

## Lines

`KINIT: ready latency class=…` — how long a runnable task waited for a hart, per class, with the
waits above 1 ms attributed (`late_idle`, `late_lower`, `late_peer`; `sched/ready_stats.rs`).
`class=reserved` is the lane of the dispatches from a reservation's budget; `le200us` of `n` is
what a time right's holder is judged by (99 of 100).
`KINIT: time rights (window_ms= reserved_max_pm= at= overruns= misses=)` — the most any hart has
reserved, and the window's overruns and misses.
`KINIT: yield top` — the three tasks that yield most (`sched/yield_stats.rs`). Both print at the
boot-end fence and in the observer's 10 s windows (`core/trap/baseline.rs`).
