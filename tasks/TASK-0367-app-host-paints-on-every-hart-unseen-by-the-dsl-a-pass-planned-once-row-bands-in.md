---
schema: 1
id: TASK-0367
title: "app-host paints on every hart, unseen by the DSL: a pass planned once, row bands in parallel, damage as rectangles, vector kernels, a UI thread that never waits"
status: in_progress
owner: ["@ui", "@runtime"]
created: 2026-11-10
updated: 2026-11-13
subsystem: ui
kind: ui
novelty: variant
estimate: 4
started: 2026-11-13
dates_source: recorded
goal:
  metric: "raster cost per painted pixel and app warm launch on the board"
  baseline: "100-355 ns per row pixel on one hart; a full window 218-501 ms for 1.2-1.4 Mpx; ux launch first_frame 570-2023 ms against 300 (board--2026-11-10T14-55-42)"
  target: "<= 10 ns per painted pixel at 8 harts (a 1920x1080 window <= 21 ms); apphost: ux launch ok (<= 300 ms) for every system app; goldens byte-equal for 1..8 workers"
  instrument: "apphost: hops motion paint_us, rows and px; apphost: ux launch; the host goldens"
consumer: hardware-network/LU.1
phases:
  - id: P0
    story: "where a row's time goes is measured: the fixed cost per row, fills, blends, glyphs, the alpha conversion, the share of float"
    state: done
    gate: "a host bench and the board line apphost: raster split; the numbers in the ledger"
    started: 2026-11-13
    done: 2026-11-13
    evidence: build/logs/board--2026-11-13T12-18-30
    note: "built 2026-11-13: apphost: raster split (rows= px= boxes= runs= pick_us= ground_us= boxes_us= text_us= alpha_us= clock_ns=) — the process's first full surface painted with a clock read between a row's stages, the reads' cost stated and taken off (raster_split.rs, host-tested; probe/paint/split.rs). Board 2026-11-13, two cycles (board--2026-11-13T11-54-25 and T12-18-30; 8 harts, one painting; clock read 0.7-1.0 us), seven processes each, first full surface 45-169 ms: the boxes are the largest stage in most (38-93 % of the pass; 14-38 ns per surface pixel on the large windows, 263-290 ns on the small surfaces with 97 and 137 boxes = 0.6-1.2 ms a box), the ground 6-22 ns/px (8-27 ms a surface, varying between cycles), the straight-alpha conversion 2-11 ns/px and 30 ns/px (62 of 92-94 ms) on the 1920x1080 surface with 12 boxes, text at most 5 ms, picking a row's boxes 0.03-0.32 ms. So: the plan and the bands (P1, P2) divide every stage; the box painter's cost per box and the alpha conversion are the two kernels P4 takes first. The share of float is the P0 measurement of TASK-0362 (soft against hard per scene); the host bench is its paint-kernel bench (docs/graphics-ui/paint-kernels.md)"
  - id: P1
    story: "a pass is planned once into an immutable plan and any band of its rows is painted from it by any thread, byte-equal to today's painter"
    state: planned
    gate: "host: band parity over splits 1..8 and per row against today's goldens (the nexus-svg band_parity pattern)"
    started: 2026-11-13
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
    state: building
    gate: "host and QEMU: byte equality of every vector kernel with its scalar reference; board: <= 10 ns per painted pixel at 8 harts"
    started: 2026-11-13
    note: "First kernels on the board (board--2026-11-13T13-12-28 against T12-18-30, same first frames): the stroke row walk (scene_raster/src/strokes.rs, byte-equal, host 7-9x) takes the boxes stage 32.4->26.6, 35.2->27.3, 42.9->30.6 ms; the 340x600 surface of 97 boxes stays 58.4->57.9 ms (285 ns/px — not strokes; name it with the host bench next). The alpha memo as 128-bit words: greeter 1920x1080 61.6->22.6 ms (30->11 ns/px), windows 10.7->5.3, 11.8->6.1 ms. ground_us 8-24 ms per first frame unchanged (hypothesis first touch, TASK-0330 P3, not measured). Launches: 4 of 7 over 300 ms before, 3 of 7 now (385/308/310 ms). Open: vector forms of the box fills, the 97-box surface, alpha below 5 ns/px."
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
