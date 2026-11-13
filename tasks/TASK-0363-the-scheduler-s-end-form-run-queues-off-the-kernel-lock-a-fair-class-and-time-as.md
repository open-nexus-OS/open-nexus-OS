---
schema: 1
id: TASK-0363
title: "The scheduler's end form: run queues off the kernel lock, a fair class, and time as a right — a budget per period, delegated as a subset, revoked in bounded time"
status: in_progress
owner: ["@kernel-team"]
created: 2026-11-10
updated: 2026-11-13
subsystem: kernel
kind: service
novelty: first
estimate: 5
started: 2026-11-11
dates_source: recorded
goal:
  metric: "wake-to-run latency p99 of a thread holding a time right on the board; run-queue operations under the kernel lock; conservation of delegated budgets"
  baseline: "every run queue lives inside the kernel lock; strict classes with a 10 ms tick; a class is a label any service may claim; wakes ran 0.3-5.1 ms late before ADR-0096 (board cycle 83); today's histogram is TASK-0330/P0's"
  target: "wake-to-run p99 <= 200 us for a time-right holder with 8 harts loaded; 0 run-queue operations under the kernel lock; the sum of delegated budgets never above the root in 10 000 random delegate, steal and migrate steps of the host model"
  instrument: "KSELFTEST: sched wake latency line; the kernel lock's holder table; sched host tests"
consumer: TASK-0365/P1
phases:
  - id: P1
    story: "the contract is decided on a host model before kernel code: a time right is a capability holding a budget per period, it is delegated only as a subset, revoked in bounded time, admitted against the harts' capacity, and what a steal and a migration do to it is stated"
    state: done
    gate: "the scheduler RFC seeded with its three lenses; host model: test_reject_delegate_beyond_held, test_reject_admit_beyond_capacity, test_reject_run_after_revoke, conservation over 10 000 random steps; TRACK-TIME-AS-RESOURCE gate G3 answered in the RFC"
    started: 2026-11-13
    done: 2026-11-13
    evidence: "source/libs/nexus-sched/tests/time_right.rs (13 host tests green, commit be98f9ed); docs/rfcs/RFC-0120"
    note: "built 2026-11-13: RFC-0120 seeded (the contract, the three rules for a steal and a migration = the track's gate G3, revoke in one step, G1 and G2 stated); the pure core nexus-sched::time_right (no allocation, no lock) with 13 host tests green: test_reject_delegate_beyond_held, test_reject_admit_beyond_capacity, test_reject_run_after_revoke, test_reject_migrate_beyond_the_targets_capacity, conservation over 10 000 random steps (every operation exercised both ways; audit after each; no reservation above share x time + one budget). Open for P3: capacity (80 % in the tests), table sizes, the syscall surface"
  - id: P2
    story: "each hart schedules under its own lock: a wake, a steal and a switch on one hart never wait for a syscall on another"
    state: building
    gate: "the sched tests run on the host (today they run nowhere); QEMU smp lane green; board: no run-queue operation among the kernel lock's holders"
    started: 2026-11-13
    note: "opened 2026-11-13 after P1's commit (be98f9ed). The cut, three boot-proven steps. S1 BUILT 2026-11-13 (operator's approval): the run queues left the kernel lock's aggregate — per hart one bounded queue per class and the running task in fixed storage (sched/run_queue.rs, pure, no allocation) behind that hart's own lock (sched::RUN_QUEUES, a HartTable of SpinIrqLock sized in smp::size_for_machine); the lock is a leaf held for one queue operation, a second hart's queues only by try-lock; a thief judges a task under its hart's queue lock and a refused task leaves only for its home hart's queue or stays. Scheduler keeps its API, its callers still hold the kernel lock. The gate's first clause is met: 22 host tests (sched/run_queue_tests.rs; the old sched/tests.rs compiled nowhere and is deleted), QEMU 1, 2 and 8 harts green with KSELFTEST work stealing ok and steal park ok (build/logs/smp1--2026-11-13T14-14-51, smp--2026-11-13T14-18-28, smp--2026-11-13T14-20-57). Board build/logs/board--2026-11-13T14-35-33: ladder 118, 8 harts, selftests and red kinds as the cycle before, interactive wake mean 17 us max 1.6 ms, bkl held sched= 196 ms per 10 s window (144 the cycle before, another session: S1 moves no holder off the lock). S2 (needs approval): a task queued on a hart or running there is that hart's — its frame and scheduling record are touched only by the hart that dequeued it, a waker sets ready and enqueues under the target's queue lock; in S1 a stolen task is in neither queue between the victim's pop and the thief's current slot, covered only by the kernel lock every caller holds. S3 (needs approval): the dispatch (idle pick, timer preempt, yield) takes no kernel lock: pick under the own queue lock, the frame from the owned record, the space's root from the activation TASK-0330 P6 already counts. The phase's board gate (no run-queue operation among the kernel lock's holders) is S3's; the phase stays open until then"
  - id: P3
    story: "a holder of a time right runs within its budget every period, everything else shares the rest fairly by weight, and every thread's run time is metered"
    state: planned
    gate: "the host model green on the kernel's own scheduler core; KSELFTEST: sched time right ok (the budget honoured over 100 periods, the overrun counted), sched fair ok (two weights within 5 % of their ratio), sched runtime metered ok"
  - id: P4
    story: "no periodic tick: each hart's timer is armed for its next deadline and an idle hart sleeps until an event"
    state: planned
    gate: "board: timer interrupts on an idle hart <= 2 per second (100 today); wake-to-run p99 <= 200 us for a time-right holder"
  - id: P5
    story: "a woken thread runs where it runs soonest: placement by load, by its last hart and by cluster, and an idle hart pulls work"
    state: planned
    gate: "QEMU -smp 8: 8 runnable compute threads occupy 8 harts within 2 ms (KSELFTEST: sched spread ok); board: the per-hart utilisation line; test_reject_steal_against_affinity kept"
    note: "found 2026-11-12 (TASK-0362 P1, fpsoak): a pin places a thread only at its next wake. set_affinity re-clamps home_cpu; a thread that yields or is preempted is requeued on the hart it is on (yield_current -> try_enqueue), only wake/resume route to the home hart. A thread that never blocks keeps running on a hart outside its new set (two-hart run smp--2026-11-12T14-07-44: 20 pins, loads=2 — neither thread moved). The placement of this phase must hold at every requeue, not only at a wake; test: test_reject_run_outside_the_set_after_a_pin."
  - id: P0
    story: "what the scheduler is rebuilt from is known: an interactive wake's wait for a hart is attributed (behind a peer of its class on the hart it is pinned to, behind a lower class it did not preempt, or a hart asleep), and the task that yields 70 000 times a second is named"
    state: done
    gate: "board: KINIT: ready latency class=interactive carries the three causes as counts (their sum = the dispatches above 1 ms; were 2 967 of 5 380, mean 1 513 us, build/logs/board--2026-11-11T09-05-45) and KINIT: yield top names the three tasks with the most yields; a missed preemption counted above 0 is fixed before P2; the yield loop is replaced by a wait or stated as the lane's own"
    started: 2026-11-11
    done: 2026-11-13
    evidence: build/logs/board--2026-11-13T10-38-20
    note: "gate met on the board (closes in the ledger when P1 starts, rule 8): ten windows with the desktop in use, 273 295 interactive dispatches, 519 above 1 ms = 508 behind a peer on the pinned hart, 9 a hart asleep, 2 a lower class not preempted (examined before P2); yield top total=0 in use — the network owner's loop removed at its cause, the harness's ladder is the lane's own yielder; ADR-0098"
  - id: P6
    story: "a wait that carries a deadline ends at its deadline on an idle system: a waitset wait and a fence wait are woken by the timer like a receive and a send, by one deadline walk instead of three copies (two of which know only IPC), and the idle hart does not walk every task under the kernel lock on each turn"
    state: planned
    gate: "host: one deadline walk covers receive, send, waitset and fence (test per kind); KSELFTEST: wait deadline ok — a waitset wait and a fence wait with a 5 ms deadline return within 10 ms while no other task makes a syscall; board: bkl takes idle= per 10 s window below 100 000 (were 911 709, build/logs/board--2026-11-11T11-05-00)"
    note: "found 2026-11-11 while reading cycle 3: wake_expired_blocked runs only inside another task's scheduling syscall; wake_expired_ipc_deadlines (timer interrupt, cpu0 idle) wakes IpcRecv/IpcSend only. Not the cause of that cycle's freezes (TASK-0330 P6). Part of P4's ground: a tickless hart needs the one walk."
---

<!-- `nx ledger new task` copies everything below this line; the frontmatter above shows the
     planning fields (RFC-0115). Write state with `nx ledger`, never by hand. -->

## The ask

The operator, 2026-11-10: finish SMP first — every hart, the kernel lock deleted — as the end
form, piece by piece, nothing rebuilt later; and take `TRACK-TIME-AS-RESOURCE` into the lane,
because a scheduler built on class labels would be rebuilt for it. The outcome: a thread that
holds a time right runs within its budget every period on a loaded 8-hart board, and no
run-queue operation waits for the kernel lock.

## Measured before the cut

- `build/logs/board--2026-11-13T10-38-20` (the desktop in use, ten windows): 273 295 interactive
  dispatches, 519 later than 1 ms — 508 behind a peer of the same class on the hart both are
  pinned to, 9 behind a hart asleep, 2 behind a lower class. A class cannot order its own
  members; that is what a deadline is for.
- Every run queue lives inside the kernel lock (`core/trap/runtime.rs`); strict classes, a 10 ms
  tick, steal one task per millisecond (`sched/mod.rs`); `sched/tests.rs` runs nowhere.
- `build/logs/board--2026-11-11T11-05-00`: 911 709 takes of the kernel lock by idle harts per
  10 s window (the deadline walk under the lock, P6).

## Three lenses

- **Scope** — in: the time right's contract and its pure core (`nexus-sched::time_right`, P1),
  run queues under a lock per hart (P2), the fair class and reservations with metering (P3), no
  periodic tick (P4), placement (P5), one deadline walk (P6) · out: the syscall surface and the
  capability kind (seeded in P3, appended to RFC-0120), a call lending the caller's reservation
  (TASK-0330, the IPC package), who is given which share (TASK-0365 P1), the supervision path's
  reservation and exhaustion as an event (after the lock is deleted, the track's candidates
  010 and 030).
- **Invariant** — every share of the root is held exactly once, no hart is reserved past its
  capacity, nothing of a revoked right runs, a reservation runs at most `share × time + one
  budget` through any steal or migration: `test_reject_delegate_beyond_held`,
  `test_reject_admit_beyond_capacity`, `test_reject_run_after_revoke`,
  `test_reject_migrate_beyond_the_targets_capacity`, `conservation_over_10_000_random_steps`
  (`source/libs/nexus-sched/tests/time_right.rs`).
- **Contract** — RFC-0120 (seeded here; answers the track's gate G3, states G1 and G2);
  ADR-0052 (per-hart earliest deadline, steal by affinity) stays; ADR-0098 gives the order the
  kernel lock is split in.

## Not doing here

- No guarantee marker before the kernel lock is deleted (gate G1): until then the mechanism's
  lines are measurements — the address-space package of TASK-0330 closes it.
- No slice table, no global deadline queue (RFC-0120, alternatives).
- No frequency or thermal control (the seam is the per-hart utilisation line of P5; the
  regulation is its own task).
- The exact release time of a hart a reservation left (its zero-lag point instead of its
  deadline): measured in P5 only if moves prove frequent.

## Definition of Done

- Every phase `done` with its evidence; the goal's target reached on the instrument.
- Host tests, the QEMU lanes and the board cycle the phases name are green.
- Security-relevant work has its `test_reject_*` and hardening markers
  (`docs/standards/SECURITY_STANDARDS.md`).
- Docs swept: CHANGELOG, affected docs, the RFC's status; `active_days` and, when the estimate
  missed, a `variance_note`.
