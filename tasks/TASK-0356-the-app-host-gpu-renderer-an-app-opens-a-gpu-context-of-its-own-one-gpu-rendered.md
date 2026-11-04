---
schema: 1
id: TASK-0356
title: "The app-host GPU renderer: an app opens a GPU context of its own, one GPU-rendered frame presented, its footprint includes the GPU buffers"
status: draft
owner: ["@ui", "@gpu"]
created: 2026-11-04
updated: 2026-11-04
subsystem: ui
kind: service
novelty: first
goal:
  metric: "GPU-rendered apps"
  baseline: "0"
  target: "1 app presents a GPU-rendered frame on QEMU virgl and on the board, its footprint read with its GPU buffers"
  instrument: "the app-host launch line and memd's footprint line"
consumer: TASK-0351/P4
phases:
  - id: P0
    story: "gpud opens a context for an app at normal priority when its manifest's capability is provisioned (ADR-0095), refuses every other sender, and destroys it with the owner"
    state: planned
    gate: "host: test_reject_a_context_without_the_capability, test_reject_a_context_beyond_the_app_s_footprint; QEMU virgl: one app's GPU-rendered frame presented"
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
