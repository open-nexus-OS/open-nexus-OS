---
schema: 1
id: TASK-0351
title: "Layer 8 on the GPU: an occluded window costs nothing, bindings grow, a fullscreen app is scanned out directly and apps render with the GPU"
status: in_progress
owner: ["@ui", "@gpu"]
created: 2026-11-03
updated: 2026-11-03
subsystem: ui
components: [gpud, windowd, nexus-gfx, app-host]
kind: service
novelty: first
estimate: 3
started: 2026-11-03
dates_source: recorded
goal:
  metric: "windows one frame can hold; GPU bindings per board context; apps that render with the GPU; GPU ns per textured pixel; frame p99 and drops on the board"
  baseline: "64 layers per frame (topmost 62 windows, occluded bodies still drawn); 128 bindings per board GPU context; 0 GPU-rendered apps; no direct scanout (TASK-0348 'Not doing here'); 13 ns/px median, 25 ms GPU frame median (cycle 61) against the stock compositor's 1.9-3.7 ns/px"
  target: "occluded windows cost 0 draws; bindings grow past 128; a fullscreen app scanned out without a compose; one app renders with the GPU; <= 4 ns/px median; frame p99 <= 16.7 ms with glass and 0 of 32 frames dropped during a transition"
  instrument: "gpud: gpu gate scene ok · gpud: gpu work (ns_per_px=) · windowd frame line (drawn=, occluded=) · ux frame p99 line"
consumer: TASK-0336
depends_on: [TASK-0348]
links:
  - label: "Scope from"
    target: "TASK-0348 'Not doing here' (the old number TASK-0350 there means this task)"
  - label: "Programs from"
    target: "TASK-0335 P4's rest (the blur's dual filter, the SDF at the edges only, the shadow as a ring), parked to layer 8 by the operator 2026-10-22"
phases:
  - id: P0
    story: "the frame cost with 12 and 30 windows and the binding use are measured on the board"
    state: proving
    gate: "board cycle: frame p99, ns_per_px, layers per frame and bindings in use logged for 12 and 30 windows"
    started: 2026-11-03
    note: "board--2026-11-03T19-13-30 measured 4 windows (frame drops 24 of 32 median, 6 ns/px, 16 layers, 49 of 128 bindings, 1 context); the 12- and 30-window series is still owed: first board cycle of package 3"
  - id: P1
    story: "a window hidden behind others costs the frame nothing"
    state: planned
    gate: "QEMU many-windows with 30 windows: occluded bodies drawn = 0; frame p99 not above the 12-window value"
  - id: P2
    story: "a board GPU context binds as many surfaces as the windows need"
    state: planned
    gate: "host test binds 129+ objects; board: 30 windows with 0 binding failures"
  - id: P3
    story: "a fullscreen opaque app goes to the display plane and the compositor skips its pass"
    state: planned
    gate: "board: compose passes = 0 while a fullscreen app presents; gpud direct scanout line"
  - id: P4
    story: "an app renders its surface with the GPU, its GPU memory charged to it"
    state: planned
    gate: "QEMU virgl and board: one app's GPU-rendered frame presented, its footprint includes the GPU buffers"
  - id: P5
    story: "a textured pixel costs what the stock compositor's does: the blur's dual filter, the SDF at the rounded edges only, the shadow as a ring"
    state: planned
    gate: "board: gpud: gpu work ns_per_px <= 4 on the median (cycle 61: 13; stock 1.9-3.7); host golden: every cut draws the same pixels"
  - id: P6
    story: "the passes a frame runs hold the budget on the board, with glass and while something moves"
    state: planned
    gate: "board: gpud: gpu gate scene ok, frame p99 <= 16.7 ms with glass on the operator series, 0 of 32 frames dropped during a transition"
---

## The ask

## Measured before the cut

- Cycle 61 (2026-10-22): 13 ns/px median, 25 ms GPU frame median; the stock compositor 1.9–3.7 ns/px.
- P0, board 2026-11-03 (`board--2026-11-03T19-13-30`, `docs/board/measurements/2026-11-03-motion-input-frame-baseline/`),
  **4 windows at most** (the 12- and 30-window series is still owed — it opens package 3):
  `ux frame` 26 FAIL / 1 warn of 27 windows, dropped 24 of 32 median, worst frame 102 ms median;
  `gpu work` 6 ns/px median over 2.8 Mpx slot area median, `layers` median 16, max 19 of 64;
  `bindings` in_use max 49 of 128 in 1 context; `windowd: hops frame` `sent>acked` 90 ms mean.
  The tap's dominant hop (TASK-0355 P0) is this frame.

## Three lenses

## Not doing here

## Definition of Done
