---
schema: 1
id: TASK-0354
title: "Motion on the compositor — an app states a transition once, windowd computes it for the time each frame is shown, springs solved in closed form and retargeted with their velocity"
status: draft
owner: ["@ui"]
created: 2026-11-03
subsystem: ui
components: [windowd, app-host, animation]
kind: ui
novelty: variant
estimate: 3
goal:
  metric: "app-host wake-ups per frame of a Tier-2 transition; frames dropped per transition (32-frame window); closed-form spring deviation from RK4"
  baseline: "1 app-host tick and 1 CPU raster per frame (Choreographer pulse, paint_row_picked_animated); drops per transition not measured; RK4 fixed-step springs restart on a retarget"
  target: "0 app-host wake-ups between a transition's start and end; 0 of 32 frames dropped per transition on the board; closed-form within 0.5 px of RK4; velocity continuous on a retarget"
  instrument: "apphost wake counter per transition · windowd frames(moved=) line · gpud: ux frame line · host tests"
consumer: TASK-0353
links:
  - label: Model
    target: "docs/dev/ui/foundations/animation.md (Tier 1/Tier 2; 'motion as layer properties is the follow-on')"
  - label: "Shown time"
    target: "TASK-0343 / RFC-0110 (the predicted landing the motion is computed for)"
  - label: Budget
    target: "RFC-0108 (frame line)"
phases:
  - id: P0
    story: "a spring is solved in closed form at any time, and a retarget keeps its velocity"
    state: planned
    gate: "host: max deviation closed-form vs RK4 <= 0.5 px for every token; velocity jump on retarget = 0"
  - id: P1
    story: "an app states a value change or transition once, and windowd moves the layer for the time each frame is shown"
    state: planned
    gate: "board: apphost wake-ups during a transition = 0; app CPU raster per transition frame = 0"
  - id: P2
    story: "the motion tokens are springs (response, damping), and reduced motion still resolves to the end state"
    state: planned
    gate: "host goldens sampled at named times for all 9 tokens; reduced motion = 0 frames of motion"
  - id: P3
    story: "every shell transition holds the frame on the board"
    state: planned
    gate: "board: panel, launcher and window open/close each 0 of 32 frames dropped, gpud: ux frame ok"
---

## The ask

Operator, 2026-11-03: the animation model as smooth as the reference class (iOS: the render server interpolates, the app is out of the frame loop) and efficient on the BPI-F3's small cores.

## Measured before the cut

- Tier 1 (window transform) already runs in windowd/GPU without a re-raster (TASK-0336, TASK-0338).
- Tier 2 (in-surface `.animate`/`.transition`/`.effect`) ticks the app-host on every frame pulse and re-rasters on the CPU (`scene_raster/src/anim.rs`).
- To measure in P1: app-host wake-ups and CPU ms per transition frame on the board.

## Three lenses

## Not doing here

- The GPU's per-pixel cost (TASK-0351), the landing prediction's accuracy (TASK-0343).

## Definition of Done
