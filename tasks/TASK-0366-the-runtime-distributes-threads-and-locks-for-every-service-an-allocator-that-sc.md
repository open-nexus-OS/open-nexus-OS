---
schema: 1
id: TASK-0366
title: "The runtime distributes: threads and locks for every service, an allocator that scales and frees, the workpool sized by the topology"
status: draft
owner: ["@runtime"]
created: 2026-11-10
updated: 2026-11-10
subsystem: app-platform
kind: service
novelty: variant
estimate: 3
goal:
  metric: "allocation rate with 8 threads against 1; speedup of a parallel loop at 8 harts; heap growth over alloc/free cycles"
  baseline: "one spin lock around a bump allocator that never frees (nexus-service-entry); MAX_WORKERS = 4; workers woken through fences; no thread, mutex or condition API for services"
  target: "8 threads allocate >= 6x the rate of 1; a parallel loop over 8 harts >= 6x on the board; the heap steady over 10 000 alloc/free cycles; an uncontended lock 0 syscalls"
  instrument: "SELFTEST: alloc scale ok, heap steady ok, workpool scale ok; host tests and miri"
consumer: TASK-0367/P2
phases:
  - id: P1
    story: "a service spawns a thread, locks a mutex and waits on a condition like any Rust program, and an uncontended lock costs no syscall"
    state: planned
    gate: "host tests of the one API over host threads; SELFTEST: thread lib ok; the uncontended path's syscalls counted 0"
  - id: P2
    story: "allocation scales with threads and memory is freed: per-thread caches by size class over the service heap"
    state: planned
    gate: "host: allocator model tests and miri; SELFTEST: alloc scale ok (8 threads >= 6x), heap steady ok; the alloc and memset soaks green"
  - id: P3
    story: "the workpool has as many workers as the topology offers, wakes them through a word, splits deterministically and runs inside its caller's time right"
    state: planned
    gate: "host equality matrix workers 1..8; SELFTEST: workpool determinism ok, workpool scale ok (>= 6x at 8 harts on the board); MAX_WORKERS retired"
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
