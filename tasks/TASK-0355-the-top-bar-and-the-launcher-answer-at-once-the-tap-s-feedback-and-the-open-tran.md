---
schema: 1
id: TASK-0355
title: "The top bar and the launcher answer at once — the tap's feedback and the open transition measured, then cut to the budget"
status: in_progress
owner: ["@ui"]
created: 2026-11-03
updated: 2026-11-05
subsystem: ui
components: [desktop-shell, app-host, windowd]
kind: ui
novelty: variant
estimate: 3
started: 2026-11-03
dates_source: recorded
goal:
  metric: "tap to the first changed frame; frames dropped while a top-bar panel or the launcher opens and while the launcher pages"
  baseline: "board 2026-11-03 (board--2026-11-03T19-13-30): tap to submit 77 ms median (15 ok, 20 warn, 20 FAIL of 55 windows); 24 of 32 frames dropped (median); dominant hop gpud sent>acked 90 ms mean, staging 0.06 ms"
  target: "tap to the first changed frame <= 50 ms; 0 of 32 frames dropped while opening a panel, the launcher (small and big) and paging; ux frame p99 <= 16.7 ms"
  instrument: "hops lines (RFC-0108 §hops) · gpud: ux frame · windowd: ux input · board-visual ack"
consumer: TASK-0353
links:
  - label: "Top bar"
    target: "TASK-0312 (topbar + six drop-downs)"
  - label: Launcher
    target: "TASK-0313 (launcher, pager glide, docks)"
  - label: Budget
    target: RFC-0108
phases:
  - id: P0
    story: "the time from a tap on the top bar or the launcher to its frame is measured on the board, hop by hop"
    state: done
    gate: "board cycle (shared with TASK-0351 P0 and TASK-0354 P1): tap-to-frame and frame p99 logged for the 6 panels, the launcher small and big and the pager; the dominant hop named in ms"
    started: 2026-11-03
    done: 2026-11-03
    evidence: build/logs/board--2026-11-03T19-13-30
  - id: P1
    story: "the dominant cost P0 names is cut"
    state: blocked
    gate: "board: the named hop at most half of its P0 value; tap to frame <= 100 ms"
    note: "re-measured on the board 2026-11-05T13-12-18 after TASK-0354 P2: a panel's open costs one layer raster in app-host (Control Center 486 ms, launcher 599 ms, a top-bar panel 283 ms), now the dominant hop ahead of gpud's frame (TASK-0351 P5/P6); the operator put TASK-0353 first"
  - id: P2
    story: "the top bar and the launcher open and page without a hitch"
    state: planned
    gate: "board: tap to frame <= 50 ms; 0 of 32 frames dropped on every open and on paging"
---

## The ask

Operator, 2026-11-03: the top bar and the launcher are very slow.

## Measured before the cut

P0, board 2026-11-03 (`board--2026-11-03T19-13-30`, `docs/board/measurements/2026-11-03-motion-input-frame-baseline/`),
the six panels, the launcher small and big, paging, windows opened and closed:

- tap to submit 77 ms median over 55 `ux input` windows (15 ok, 20 warn, 20 FAIL); the frame
  24 of 32 dropped (median), worst frame 102 ms median.
- **The dominant hop is gpud's frame**: `windowd: hops frame` `sent>acked` 90 ms mean (worst
  396 ms) against `owed>sent` 16 ms and the staging stage `staged>applied` 55 µs mean (worst
  8.8 ms). The input path in windowd is not the cost; the display's frame is — its owner is
  TASK-0351 (the passes, the per-pixel cost: 6 ns/px median over ~2.8 Mpx of slot area, 16
  layers). P1 is blocked on that cut.
- The shell's own transition frames cost 77 ms each on the app side (TASK-0354 P1's line) — the
  second cost, removed by TASK-0354 P2.
- Not separable from the lines: which panel or launcher size a tap opened. The ux lines are
  windows of 16 inputs, not taps; a per-control attribution needs windowd's motion record
  (TASK-0354 P2).

Before the cycle the suspects were the Control Center's full-screen glass re-blurred on any damage
(TASK-0335 context), the shell's CPU raster per transition frame, the pager glide on ScrollMomentum X.

## Three lenses (architecture-review, 2026-11-03)

- **Scope** — P0 is measurement only, from the lines that exist (`windowd: hops frame`, `gpud: ux
  frame`, `windowd: ux input`, `apphost: ux launch`), in the one board cycle shared with TASK-0351
  P0 and TASK-0354 P1. P1 cuts the hop P0 names, in that hop's owner: the shell's DSL (`userspace/
  apps/desktop-shell`), app-host's raster, windowd's compose, or gpud's passes. Out: a new mechanism
  before P0 names the hop; the motion mechanics (TASK-0354); the GPU's per-pixel cost (TASK-0351);
  the panels' pixels (TASK-0312, TASK-0313 stay the fidelity contract).
- **Invariant** — every hop is stamped from the kernel's clock on the receiving side, never from the
  app's claim; no new marker says `ok` before the budget is met (no-fake-green: a tap-to-frame line
  carries its verdict from `nexus-ux-budgets`); the numbers are read against
  `docs/dev/ui/foundations/quality/latency-reference.md` (tap <= 50 ms, 16.7 ms frame). Negative
  test: P0 adds none (it adds no mechanism); P1's cut carries its own `test_reject_*` by its owner.
- **Contract** — RFC-0108 (the budgets, the hops line format); RFC-0110 (the shown time the frame
  is judged by). No wire change in P0; P1 names its contract once the hop is known.

## Not doing here

- The general GPU per-pixel cost (TASK-0351); the motion model (TASK-0354); the design deltas
  (TASK-0353).

## Definition of Done
