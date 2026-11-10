---
schema: 1
id: TASK-0365
title: "Placement by load: the display chain holds a time right, no program names a hart, and an exhausted budget is an event"
status: draft
owner: ["@kernel-team", "@runtime"]
created: 2026-11-10
updated: 2026-11-10
subsystem: kernel
kind: service
novelty: variant
estimate: 3
goal:
  metric: "harts the display chain runs on within one frame; literal CPU masks in userspace; the compositor's wait from owing a frame to sending it"
  baseline: "the chain (gpud, windowd, inputd, hidrawd, xhcid, touchd, imed) is pinned to hart 0; literal masks in init affinity.rs, execd sched_recipe.rs, nexus-workpool and pinched; windowd hops frame owed>sent mean 11-27 ms, max 92-321 ms (board--2026-11-10T14-55-42)"
  target: "the chain on >= 3 harts within one frame; 0 literal masks (retired-name gate); owed>sent mean <= 2 ms on the board"
  instrument: "the scheduler's per-hart utilisation line; windowd: hops frame; the retired-name gate"
consumer: hardware-network/LU.1
phases:
  - id: P1
    story: "init derives the display chain's time right and hands it down: windowd, gpud, inputd and app-host hold a share, and app-host gives an app only what it holds"
    state: planned
    gate: "init: time rights line (root, delegated, sum <= root); test_reject_an_app_claims_more_than_app_host_holds; the soft-realtime list is one table for the right and the class"
  - id: P2
    story: "no program names a hart: service placement, the sched recipes and the worker counts of the workpool and pinched come from the exported topology and from roles"
    state: planned
    gate: "retired-name gate for the literal masks and MAX_WORKERS; the one-hart lane green (the same roles on one hart)"
  - id: P3
    story: "a frame's threads run side by side: the chain on several harts of one cluster, background work on the other"
    state: planned
    gate: "board: >= 3 harts busy with the chain in a frame window (utilisation line); windowd: hops frame owed>sent mean <= 2 ms; gpud: ux frame ok in the idle-desktop windows"
  - id: P4
    story: "the supervision path holds a time right of its own and an exhausted budget is an event its supervisor receives"
    state: planned
    gate: "SELFTEST: supervision right ok (a spinning service delays a restart by at most one period); SELFTEST: budget exhausted event ok; TRACK-TIME-AS-RESOURCE candidates 010-030 closed"
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
