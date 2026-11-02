---
schema: 1
id: TASK-0345
title: "The compositor stalls with four quadrant tiles and a modal while the GPU is overloaded — one frame a second, the pointer frozen"
status: draft
status_note: "found 2026-10-23, board cycle 68; filed by the operator as its own defect, not a 6c-0 blocker"
owner: ["@ui", "@gpu"]
created: 2026-10-23
subsystem: ui
links:
  - label: Evidence
    target: "build/logs/board--2026-10-23T17-18-42/uart.log (cycle 68, build dev-a0d5fb43)"
  - label: "Found during"
    target: "tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md (6c-0, the OS build's own flags)"
  - label: Measurement
    target: "docs/board/measurements/2026-10-23-os-build-flags/README.md (§ cycle 68)"
---

## Context (board cycle 68)

The operator: "the whole system hung" until the reset. The trace (no process died, no fault
beyond the selftests' own `fault-probe` and `pinched: selftest crash requested`):

- **The scene:** four app windows tiled into the four quadrants by drag (`windowd: wm tile
  (zone=top-left|top-right|bottom-left|bottom-right …)`, lines 2527–2533) — the first board cycle
  that ever tiled four quadrants — then a tap at the shell's top right opened the control
  center as a window modal (`apphost: modal open (depth=1)`, `windowd: win modal on (id=4)`,
  line 3022–3025).
- **The GPU was overloaded before it:** `gpud: gpu frame us (n=32 avg=64018 max=149569 …
  passes=7.7 draws=172 glass_w=72 glass_r=117 … wait 40035)` and `gpud: ux frame FAIL
  (dropped=31 of=32)` (line 3004; cycle 67's frame average median 24.5 ms).
- **Then the compositor nearly stopped:** from line 3085 to the log's end every
  `windowd: loop` window shows `present=1 … frames(content=0 blit=1 pointer=0 moved=0 …)` with
  `sent>acked` ≈ 135 ms, while inputd reports the mouse travelling hundreds of pixels per window
  (`inputd: hid rx hz=… travel dx=599 …`) and app-host still receives taps
  (`apphost: tap (1896,17) hit=Some(198)`, line 3135). The pointer did not move on screen.
- Around it: `WINDOWD: FAIL imed relay send` / `inputd: imed send FAIL` at each text focus (the
  known keyboard defect, cycle 67 too), one `windowd: flush rows fail` (line 2922; seen once
  before, `board--2026-10-20T13-38-05`) with `windowd: ux input FAIL (to_submit_us=140546)`.
- Across every October board log, this is the only one that ENDS in a run of
  `present≤1 … moved=0` windows with the mouse moving (earlier runs of three recovered).

## Open questions (to measure, not to assume)

1. Why the pointer stops: the pointer is the controller's layer — what gates `moved=` behind the
   frame chain while a frame is in flight?
2. Why one present a second at a 135 ms ack: what windowd waits for between presents.
3. Whether the scene reproduces on the build without 6c-0's flags (an A/B on the same series) —
   6c-0 changes instruction selection only; a codegen fault is possible, not likely.
4. The GPU's cost of four glass windows + a modal backdrop (`glass_w=72`, blur per window).

## Stop conditions

The same series (four quadrant tiles, the control center opened and dismissed, the mouse moving)
on the board: the pointer follows the mouse at once throughout, no `windowd: loop` window with
`moved=0` while `travel` > 0, the compositor never below the frame pace's floor; a host test for
the gate found in (1).

## Seen since

- Board cycle 69 (`board--2026-10-25T12-17-03`, build dev-c51e5c0d, 6c-1a): four quadrant tiles
  again (`wm tile` top-left/top-right/bottom-left/bottom-right) and three modals — **no stall**
  (the longest `moved=0` run two windows, recovered), gpud's frame average median 31 ms. Whether
  the control center was opened while all four were tiled is not known from the trace; the
  question stays open.
