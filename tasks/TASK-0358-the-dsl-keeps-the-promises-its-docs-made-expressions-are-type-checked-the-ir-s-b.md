---
schema: 1
id: TASK-0358
title: "The DSL keeps the promises its docs made: expressions are type-checked, the IR's budgets are enforced, the CLI works on app directories"
status: draft
owner: ["@ui"]
created: 2026-11-09
updated: 2026-11-09
subsystem: devx
kind: hardening
phases:
  - id: P0
    story: "every expression has a type the checker knows: a binary operator, a call argument, a reducer assignment and a condition are checked against the declared field, prop and event types, and a mismatch is a compile error"
    state: planned
    gate: "host: dsl-core tests — Int + Str, assigning a Str to an Int field and a non-Bool condition are each rejected (test_reject_*), every shipped app and every docs example still compiles"
  - id: P1
    story: "the IR's budgets hold at runtime: maxListLen, maxStrLen, maxExprNodes and maxEffectSteps are enforced when a program runs, with a deterministic error instead of an unbounded list, string or loop"
    state: planned
    gate: "host: runtime tests push a list and a string past their budget and an effect past its steps and get the budget error (test_reject_*), apps unchanged"
  - id: P2
    story: "the CLI and the toolchain say what they do: lint, check, fmt and hash take an app directory like build does; the IR schema header names its real version; NX0203 has one meaning; an import is either resolved or rejected"
    state: planned
    gate: "host: cli tests for each verb on examples/dsl/masterdetail; ui_ir.capnp header at the writer's version; a stale import is a diagnostic"
  - id: P3
    story: "app-host verifies the program it runs: the programHash is recomputed at mount (hash-verify on in the app-host build) and windowd drops the DSL crates it does not use"
    state: planned
    gate: "host: a payload with a flipped byte is refused at mount (test_reject_*); windowd builds without nexus-dsl-runtime and nexus-dsl-ir"
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
