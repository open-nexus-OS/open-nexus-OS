---
schema: 1
id: TASK-0369
title: "Pictures: PNG and JPEG decoded on every hart to the size shown, tiles for zoom, cached thumbnails, the DSL's Image from a file — and the Images app of the design artifacts"
status: draft
owner: ["@ui"]
created: 2026-11-10
updated: 2026-11-10
subsystem: ui
kind: ui
novelty: first
estimate: 5
goal:
  metric: "time from opening a 24 MP JPEG to its fitted picture on the board; frames dropped while zooming; peak memory against the pixels shown"
  baseline: "the OS decodes no picture format (0); the DSL's Image shows baked assets only; TASK-0090 and TASK-0082 are drafts"
  target: "<= 300 ms to the fitted picture; 0 of 32 frames dropped while zooming 25-400 %; peak memory <= 4x the pixels shown; every malformed or oversized file refused"
  instrument: "images: open line (decode_ms, tiles, peak_kb); gpud: ux frame; the host conformance and fuzz corpus"
consumer: TASK-0361/P2
phases:
  - id: P0
    story: "the design's Images screen and its photo set are the contract, the picture pipeline is specified, and decode costs are measured on the host for the reference sizes"
    state: planned
    gate: "the picture-pipeline RFC seeded with three lenses; a host bench table (PNG, JPEG baseline and progressive, up to 24 MP) in the ledger"
  - id: P1
    story: "a PNG and a JPEG are decoded by our own decoders, row by row, refusing what is malformed or too large"
    state: planned
    gate: "host: the conformance corpus byte-equal to its reference decodes; 10 minutes of fuzzing without a panic; test_reject for a decompression bomb, a truncated file, a bad checksum and an oversized picture"
  - id: P2
    story: "a picture is decoded to the size it is shown at, on every hart, beside the UI thread: a job to pinched, the result a surface charged to the app"
    state: planned
    gate: "board: a 24 MP JPEG fitted into the 900x580 window <= 300 ms; peak memory <= 4x the pixels shown; memd's footprint line includes the surface"
  - id: P3
    story: "zooming shows tiles: only the visible region is decoded at the scale needed and what is off screen is given back under pressure"
    state: planned
    gate: "board: 0 of 32 frames dropped while zooming 25-400 %; memd: the tiles purged at a pressure level"
  - id: P4
    story: "the DSL's Image takes a file: a placeholder at once, the picture when it is decoded, never a blocked frame - one line for the developer"
    state: planned
    gate: "host: dsl_apps_conformance; docs/dev/dsl updated in the same run; board: the UI thread's longest block <= 2 ms"
  - id: P5
    story: "the Images app of the design artifacts: the window, the menu, the header with name, dimensions and size, the open dialog's grid, the zoom bar with fit and 1:1, the info dialog"
    state: planned
    gate: "board-visual: images in light and dark; the app structure gate green (README, tests, i18n)"
  - id: P6
    story: "Stash shows thumbnails from a cache keyed by content"
    state: planned
    gate: "board: a folder of 100 pictures shows its thumbnails <= 1 s cold and <= 100 ms warm; the cache purgeable"
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
