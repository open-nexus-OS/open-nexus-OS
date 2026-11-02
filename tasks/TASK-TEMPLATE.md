---
schema: 1
id: TASK-XXXX
title: "What the user or a service sees when this is done"
status: draft
owner: ["@runtime"]
created: YYYY-MM-DD
subsystem: ui
kind: service
novelty: first
estimate: 3
goal:
  metric: "the one number this task moves"
  baseline: "where it is today, measured (and where)"
  target: "where it must be"
  instrument: "the marker or command that measures it"
consumer: "who uses what this builds, in this milestone"
depends_on: []
implements: []
phases:
  - id: P0
    story: "the baseline is measured before anything is built"
    state: planned
    gate: "a measurement folder with the series and a one-line verdict"
  - id: P1
    story: "one sentence of behaviour a user or a service sees"
    state: planned
    gate: "the stop condition with its proof, and the value it must reach"
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
