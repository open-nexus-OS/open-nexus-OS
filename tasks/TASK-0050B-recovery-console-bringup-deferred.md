---
title: TASK-0050B Recovery bringup console (recovery-sh) — DEFERRED placeholder
status: Deferred
owner: @reliability
created: 2026-08-18
depends-on:
  - TASK-0050 # boot targets (the graph this console would run in)
  - TASK-0051 # ops surface (the ONLY operations it may call)
follow-up-tasks: []
links:
  - Reliability contract: docs/rfcs/RFC-0087-reliability-failure-model-v1.md
  - Authority registry: tasks/TRACK-AUTHORITY-NAMING.md
---

## Status: Deferred by decision (2026-08-18)

The consumer-grade end state needs no interactive recovery shell: recovery is a
reduced declarative service graph (TASK-0050), operations run over policy-gated
IPC ops + `nx` subcommands (TASK-0051), and evidence collection is `nx diagnose`.
A console is a *bringup tool* for the day a target exists without a host-side
channel (real hardware bringup). Ledger exists so the idea has one home and
cannot re-enter other ledgers as scope drift.

## Scope when (if) activated

- A bounded UART line editor running only in the `recovery` target's graph.
- **Built-ins are thin verbs over the TASK-0051 ops surface** — the console owns
  ZERO operation logic, no second fsck/slot/diag semantics, no second command
  language beyond verb names mirroring `nx recovery` verbs.
- Mutating verbs require `.nxra` tokens exactly like the IPC surface (TASK-0053)
  — the console is a client, not a bypass.
- No arbitrary execution, no escape hatch; built-ins only.

## Activation gate

Activate only when a concrete bringup scenario without host tooling exists
(named board / channel). Until then: any PR adding console code is drift.
