---
schema: 1
id: TASK-0363
title: "The scheduler's end form: run queues off the kernel lock, a fair class, and time as a right — a budget per period, delegated as a subset, revoked in bounded time"
status: draft
owner: ["@kernel-team"]
created: 2026-11-10
updated: 2026-11-10
subsystem: kernel
kind: service
novelty: first
estimate: 5
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
