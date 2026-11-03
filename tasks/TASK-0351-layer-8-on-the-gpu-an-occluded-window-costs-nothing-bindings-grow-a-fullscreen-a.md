---
schema: 1
id: TASK-0351
title: "Layer 8 on the GPU: an occluded window costs nothing, bindings grow, a fullscreen app is scanned out directly and apps render with the GPU"
status: draft
owner: ["@ui", "@gpu"]
created: 2026-11-03
updated: 2026-11-03
subsystem: ui
components: [gpud, windowd, nexus-gfx, app-host]
kind: service
novelty: first
estimate: 3
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
    state: planned
    gate: "board cycle: frame p99, ns_per_px, layers per frame and bindings in use logged for 12 and 30 windows"
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

## Three lenses

## Not doing here

## Definition of Done
