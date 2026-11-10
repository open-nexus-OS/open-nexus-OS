---
schema: 1
id: TASK-0364
title: "Threads of a process: shared capabilities, a futex with an owner, thread-local storage, guarded stacks and join"
status: draft
owner: ["@kernel-team", "@runtime"]
created: 2026-11-10
updated: 2026-11-10
subsystem: kernel
kind: service
novelty: first
estimate: 3
goal:
  metric: "thread and synchronisation primitives of the ABI; syscalls of an uncontended lock"
  baseline: "a thread has an empty capability table, no storage of its own and a caller-provided stack without a guard page; there is no way to wait on a word; the one user is nexus-workpool"
  target: "a process's threads share its capability table; wait, wake and requeue on a word with owner inheritance; thread-local storage; guarded stacks; join - each with its test_reject; an uncontended lock costs 0 syscalls"
  instrument: "SELFTEST: thread caps shared ok, futex ok, futex pi ok, thread tls ok, thread stack guard ok, thread join ok; host ABI tests"
consumer: TASK-0366/P1
phases:
  - id: P1
    story: "the contract: a process owns its capabilities and its threads share them, a worker may be spawned without any; the wait-on-a-word ABI, thread-local storage and thread stacks are specified with their refusals"
    state: planned
    gate: "the threads RFC and the ADR 'a process owns its capabilities' seeded with three lenses; every test_reject named in the RFC"
  - id: P2
    story: "a process's threads use its capabilities under its one identity, and a compute worker spawned without capabilities can reach nothing"
    state: planned
    gate: "SELFTEST: thread caps shared ok (the receiver sees the process's service id); test_reject_ipc_from_a_capless_thread; process exit takes threads ok kept"
  - id: P3
    story: "a thread waits on a word and is woken through it, with a deadline, and the owner of a contended lock inherits its waiter's right to run"
    state: planned
    gate: "host model tests; SELFTEST: futex ok (wait, wake n, requeue, timeout), futex pi ok; test_reject_futex_on_unmapped_word, test_reject_wake_in_a_foreign_space"
  - id: P4
    story: "a thread has storage of its own, a stack with a guard page from the kernel's address allocator, and its end is awaited"
    state: planned
    gate: "SELFTEST: thread tls ok, thread stack guard ok (an overflow is the thread's fault with its name, never silent corruption), thread join ok"
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
