---
schema: 1
id: TASK-0367
title: "app-host paints on every hart, unseen by the DSL: a pass planned once, row bands in parallel, damage as rectangles, vector kernels, a UI thread that never waits"
status: in_progress
owner: ["@ui", "@runtime"]
created: 2026-11-10
updated: 2026-11-19
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
    state: done
    gate: "host: band parity over splits 1..8 and per row against today's goldens (the nexus-svg band_parity pattern)"
    started: 2026-11-13
    done: 2026-11-16
    evidence: build/logs/board--2026-11-16T09-14-28
    note: "built 2026-11-16: PassPlan (app-host src/pass_plan.rs, NOT target-gated, Sync): borrowed boxes, picks, animation slots, text runs, scroll views; anim snapshot, caret, width; the row painter moved verbatim out of probe/paint.rs; paint_ground_row (the base row) and paint_ground_band (test-gated until P2's worker calls it). Host pass_plan_tests.rs: 1..8 bands on threads, reverse over garbage, every row alone, byte-equal; the glyph pass draws (embedded atlas as dev-dependency). QEMU visible and usb-visible green (32/32 steps). Board build/logs/board--2026-11-16T09-14-28: ladder 118, 8 harts, red kinds as before, first frames unchanged (calculator boxes 22.3-23.3 ms). Closes with the commit (rule 8)"
  - id: P2
    story: "the bands of a pass are painted on every hart the workpool has, inside the frame's time right, and the DSL sees none of it"
    state: planned
    gate: "board: ns per painted pixel <= one fifth of P0's at 8 harts; goldens byte-equal; no DSL source changed"
    note: "needs the runtime's workpool v2 (TASK-0366: workers from the topology, a safe job API over the PassPlan, joining the caller's frame) on threads of a process (TASK-0364, kernel). The workpool v1 (nexus-workpool: 4 workers, static stacks, an extern C job with a raw context) would need unsafe in app-host and be rebuilt — not built on it. P1's plan is the band source P2 hands out"
  - id: P3
    story: "a row costs its damaged columns, not its width"
    state: building
    gate: "board: paint_us <= 1 ms for a 3 000 px damage (18.7 ms for 56 full rows today); host: band_math Cols tests"
    started: 2026-11-16
    note: "built 2026-11-16: redraw paints only the damage rectangle's columns (render_rows(mem, rows, Some(cols)) -> PassPlan::paint_ground_cols); picks by pass_plan::meets_cols (box, shadow reach, text halo, 2 px overhang for a glyph or a stroke cap, a view's dx; an animated box always); a motion layer that went widens to full. Host: 56 column windows byte-equal inside, untouched outside, the mutation without the overhang fails at the icon's cap. Board build/logs/board--2026-11-16T09-14-28: 17 rows x ~300 px 93-194 us a frame (913-986 before), 42 rows x ~1 800 px 0.9-1.3 ms (3.8), 32 rows x 1 056 px 0.41 ms (2.4), menu enter 3.6-9.3 ms (13.1). Gate not met for a tall narrow damage: 3 541 px over 187 rows 5.7 ms — the boxes crossing those rows still walk their whole width per row (shadow and coverage loops outside the scissor). Next: the shared painter's loops clamp to the canvas scissor (nexus-scene-raster), byte-equal by span_identity 2026-11-18 DESIGN (arch result build/agents/arch-shell-transitions-2026-11-18.md): the scissor is applied per pixel inside blend/set (scene_raster lib.rs:69-116), so shadow (lib.rs:397, ~30 us/row at 1920 px = the 5.7 ms), sprite (lib.rs:259), shapes.rs and borders.rs loops walk their whole range; cut: RowCanvas::visible(x0,x1) for contiguous loops and sees(x) before float work, no loop body changed; proof span_identity per painter against the per-pixel reference under a sweep of scissors. Cols becomes the rect set (TASK-0354 P8). BUILT 2026-11-18 (package, uncommitted): RowCanvas::visible cuts the shadow, sprite, shape and border loops to the scissor (span_identity scissor sweep byte-equal, 28 tests); the 200-edit repaint test found and fixed two column-pick errors (a large stroke's reach, an ellipsis past a narrow box: pass_plan/cols.rs). QEMU build/logs/usb-visible--2026-11-18T23-25-19: a 176-row x ~190 px damage 7.4-9.7 ms (board decides the gate). BOARD build/logs/board--2026-11-19T10-18-04: no episode with rows >= 150 and px <= 4000 in this series (the only tall episodes are whole renders: rows=988 px=0) — the gate waits for a series with a tall narrow change."
  - id: P4
    story: "fills, blends and glyph blits run on the vector unit where the hart has one, the scalar path stays the reference"
    state: building
    gate: "host and QEMU: byte equality of every vector kernel with its scalar reference; board: <= 10 ns per painted pixel at 8 harts"
    started: 2026-11-13
    note: "Kernels on the board so far. (1) Stroke row walk (strokes.rs) and the alpha memo as 128-bit words, board--2026-11-13T13-12-28: boxes 32.4->26.6, 35.2->27.3, 42.9->30.6 ms; alpha greeter 1920x1080 61.6->22.6 ms (30->11 ns/px), windows 10.7->5.3 ms. (2) Arc bands (shapes::arc_band: fill, glass reset, ring, shine, shadow ask only the columns an arc crosses; byte-equal over 60 000 rows), board--2026-11-13T13-50-14: the calculator's 340x600 first frame 57.9->22.1 ms in boxes, the other windows 24.3-26.5 ms (before 26.6-30.6). Launches over 300 ms: 5 of 10 (309-366 ms). Still open against the gate (ux launch <= 300 ms for every system app): boxes 14-26 ms a window, ground_us 6-23 ms a first frame (hypothesis first touch of the surface, TASK-0330 P3, not measured), alpha 5-7 ms a window and 22.6 ms on the greeter; vector forms of the box fills."
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
