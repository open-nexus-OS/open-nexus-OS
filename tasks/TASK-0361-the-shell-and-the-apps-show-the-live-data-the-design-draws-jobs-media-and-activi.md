---
schema: 1
id: TASK-0361
title: "The shell and the apps show the live data the design draws: jobs, media and activity, presence and the session's name, message and file metadata"
status: draft
owner: ["@ui"]
created: 2026-11-10
updated: 2026-11-10
subsystem: ui
kind: ui
novelty: variant
estimate: 4
goal:
  metric: "design deltas open because the data they show does not exist"
  baseline: "38 counted 2026-11-10 (moved from TASK-0353 P3)"
  target: "0 open; the moved ids closed"
  instrument: "the TASK-0353 audit tables re-run against the spec files"
consumer: hardware-network/LU.1
phases:
  - id: P0
    story: "the data contracts the design draws: a jobs feed, a media and activity session, presence and the session's user name, message metadata (sender, transport, time, kind), file metadata (kind, created, dimensions, hidden), volume events and the short date"
    state: planned
    gate: "the contracts reviewed (RFC or schema amendment); host tests of each codec"
  - id: P1
    story: "the shell shows them: the taskbar's jobs, the island's runner, call, navigation and timer, the now-playing card, the volume toast, the top bar's short date"
    state: planned
    gate: "host conformance tests; QEMU lane markers; board-visual"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): shell-bars AR-01 AR-02 AR-03 AR-04 AR-05 AR-06 AR-07; shell-bars JB-05; shell-bars TS-11; shell-bars TB-11; shell-bars TB-22; shell-panels PN-DOTS"
  - id: P2
    story: "the apps show them: Chat's presence and message metadata, Stash's properties, kind tiles, crumbs at any depth and multi-selection, the launcher's session name, Settings' live subtitles"
    state: planned
    gate: "host conformance tests; the moved delta ids closed in the re-run audits"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): launcher-lock D5; chat H32; chat H40 H45 H46 H60; osk 11 13 18 20 21 22 47; osk 25 35; osk 42 43 50; settings-stash S27; settings-stash F5 F15 F18; settings-stash F7; settings-stash F21 F24 F25"
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
