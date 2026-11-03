---
schema: 1
id: TASK-0350
title: "init pins every declared leg once: keystored's optional logd leg is pinned twice at boot"
status: draft
owner: ["@runtime"]
created: 2026-11-02
updated: 2026-11-02
subsystem: kernel
phases:
  - id: P1
    story: "init pins every declared leg once, so a boot names no failed route"
    state: planned
    gate: "QEMU headless and the board: 0 lines of init: FAIL declared slot route send (today 1 per boot, since 2026-10-28 at least)"
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
