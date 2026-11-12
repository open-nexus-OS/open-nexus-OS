---
schema: 1
id: TASK-0363
title: "The scheduler's end form: run queues off the kernel lock, a fair class, and time as a right — a budget per period, delegated as a subset, revoked in bounded time"
status: in_progress
owner: ["@kernel-team"]
created: 2026-11-10
updated: 2026-11-12
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
    state: planned
    gate: "the scheduler RFC seeded with its three lenses; host model: test_reject_delegate_beyond_held, test_reject_admit_beyond_capacity, test_reject_run_after_revoke, conservation over 10 000 random steps; TRACK-TIME-AS-RESOURCE gate G3 answered in the RFC"
  - id: P2
    story: "each hart schedules under its own lock: a wake, a steal and a switch on one hart never wait for a syscall on another"
    state: planned
    gate: "the sched tests run on the host (today they run nowhere); QEMU smp lane green; board: no run-queue operation among the kernel lock's holders"
    note: "from P0 (ADR-0098): the idle loop takes the kernel lock 91 000 times a second (39 % of wall time on 4 harts) — an idle hart reads only its own run queue; 2 missed preemptions in 273 295 interactive dispatches are examined first"
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
    state: proving
    gate: "board: KINIT: ready latency class=interactive carries the three causes as counts (their sum = the dispatches above 1 ms; were 2 967 of 5 380, mean 1 513 us, build/logs/board--2026-11-11T09-05-45) and KINIT: yield top names the three tasks with the most yields; a missed preemption counted above 0 is fixed before P2; the yield loop is replaced by a wait or stated as the lane's own"
    started: 2026-11-11
    evidence: build/logs/board--2026-11-11T11-05-00
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

Who asked, in their words, and the outcome they want.

## Measured before the cut

What the code and the system do today — numbers, log paths, the measurement folder. A plan
that names no measurement is a guess.

## Three lenses

- **Scope** — in: … · out: … (with where each "out" goes).
- **Invariant** — what must hold, and the `test_reject_*` that proves it.
- **Contract** — the RFC/ADR this implements or seeds.

## Not doing here

The scope firewall: what is deliberately left out, and which task or milestone step takes it.

## Definition of Done

- Every phase `done` with its evidence; the goal's target reached on the instrument.
- Host tests, the QEMU lanes and the board cycle the phases name are green.
- Security-relevant work has its `test_reject_*` and hardening markers
  (`docs/standards/SECURITY_STANDARDS.md`).
- Docs swept: CHANGELOG, affected docs, the RFC's status; `active_days` and, when the estimate
  missed, a `variance_note`.
