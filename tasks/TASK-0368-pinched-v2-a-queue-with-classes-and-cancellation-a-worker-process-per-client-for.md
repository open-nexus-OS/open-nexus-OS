---
schema: 1
id: TASK-0368
title: "pinched v2: a queue with classes and cancellation, a worker process per client for untrusted input, completion as an event, the compute-distribution seam"
status: draft
owner: ["@runtime", "@security"]
created: 2026-11-10
updated: 2026-11-10
subsystem: app-platform
kind: service
novelty: variant
estimate: 3
goal:
  metric: "jobs in flight; the broker's overhead per job; address spaces shared between clients' untrusted jobs; heap growth over jobs"
  baseline: "1 job at a time; completion by polling the job's header; a bump allocator that never frees; 2 workers; every client's job in one address space"
  target: "64 queued jobs in 3 classes; a cancel takes effect within one chunk; broker overhead <= 200 us per job on the board; 0 address spaces shared between two clients' untrusted jobs; the heap steady over 10 000 jobs"
  instrument: "SELFTEST: pinched queue ok, cancel ok, isolation ok, heap steady ok; pinched: job line"
consumer: TASK-0369/P2
phases:
  - id: P1
    story: "the contract: two tiers (the workpool inside a frame, pinched beside it), job classes, isolation domains, and the seam through which jobs travel and capabilities never"
    state: planned
    gate: "the compute-model RFC and ADR-0074 seeded with three lenses; test_reject_remote_backend_until_built named"
  - id: P2
    story: "many jobs wait in classes, the visible one first; a cancelled job stops at its next chunk; a job's memory returns when it ends; its completion arrives as an event"
    state: planned
    gate: "SELFTEST: pinched queue ok, cancel ok, heap steady ok; the wait-not-poll gate green for the client side"
  - id: P3
    story: "an untrusted job runs in a worker process of its client's own, without capabilities, started from the sealed image and frozen when idle"
    state: planned
    gate: "SELFTEST: pinched isolation ok; test_reject_worker_ipc, test_reject_cross_client_object; a warm worker start <= 20 ms on the board"
  - id: P4
    story: "the backend is chosen behind one seam, the local one runs and the remote one is refused by name"
    state: planned
    gate: "host: test_reject_remote_backend_until_built; the determinism contract over the backends"
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
