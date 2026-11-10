---
schema: 1
id: TASK-0367
title: "app-host paints on every hart, unseen by the DSL: a pass planned once, row bands in parallel, damage as rectangles, vector kernels, a UI thread that never waits"
status: draft
owner: ["@ui", "@runtime"]
created: 2026-11-10
updated: 2026-11-10
subsystem: ui
kind: ui
novelty: variant
estimate: 4
goal:
  metric: "raster cost per painted pixel and app warm launch on the board"
  baseline: "100-355 ns per row pixel on one hart; a full window 218-501 ms for 1.2-1.4 Mpx; ux launch first_frame 570-2023 ms against 300 (board--2026-11-10T14-55-42)"
  target: "<= 10 ns per painted pixel at 8 harts (a 1920x1080 window <= 21 ms); apphost: ux launch ok (<= 300 ms) for every system app; goldens byte-equal for 1..8 workers"
  instrument: "apphost: hops motion paint_us, rows and px; apphost: ux launch; the host goldens"
consumer: hardware-network/LU.1
phases:
  - id: P0
    story: "where a row's time goes is measured: the fixed cost per row, fills, blends, glyphs, the alpha conversion, the share of float"
    state: planned
    gate: "a host bench and the board line apphost: raster split; the numbers in the ledger"
  - id: P1
    story: "a pass is planned once into an immutable plan and any band of its rows is painted from it by any thread, byte-equal to today's painter"
    state: planned
    gate: "host: band parity over splits 1..8 and per row against today's goldens (the nexus-svg band_parity pattern)"
  - id: P2
    story: "the bands of a pass are painted on every hart the workpool has, inside the frame's time right, and the DSL sees none of it"
    state: planned
    gate: "board: ns per painted pixel <= one fifth of P0's at 8 harts; goldens byte-equal; no DSL source changed"
  - id: P3
    story: "a row costs its damaged columns, not its width"
    state: planned
    gate: "board: paint_us <= 1 ms for a 3 000 px damage (18.7 ms for 56 full rows today); host: band_math Cols tests"
  - id: P4
    story: "fills, blends and glyph blits run on the vector unit where the hart has one, the scalar path stays the reference"
    state: planned
    gate: "host and QEMU: byte equality of every vector kernel with its scalar reference; board: <= 10 ns per painted pixel at 8 harts"
  - id: P5
    story: "the UI thread never waits for a service: effects, file reads and decodes complete as events"
    state: planned
    gate: "board: apphost: ui thread longest block <= 2 ms over the ladder; apphost: ux launch ok (<= 300 ms) for every system app"
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
