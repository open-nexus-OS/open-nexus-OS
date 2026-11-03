---
schema: 1
id: TASK-0353
title: "The desktop matches the design of the Claude artifacts: tokens, primitives and screens, without a frame-budget regression"
status: draft
owner: ["@ui"]
created: 2026-11-03
subsystem: ui
components: [desktop-shell, app-host, windowd]
kind: ui
novelty: variant
estimate: 2
goal:
  metric: "screens of the artifacts that match on the board; design deltas left (tokens, sizes, radii, spacing)"
  baseline: "0 screens matched; deltas counted in P0"
  target: "every artifact screen matched (fixed sizes and radii exact, padding/gap on the 4px grid, as TASK-0312/0313); 0 deltas left; frame p99 not above today's"
  instrument: "board-visual ack per screen · host goldens · ux frame p99 line"
links:
  - label: Source
    target: "Claude artifacts from the operator (2026-11-03), captured into the repo in P0"
  - label: Pattern
    target: "TASK-0312, TASK-0313 (pixel-fidelity contract of the earlier handoffs)"
phases:
  - id: P0
    story: "the artifacts are the design contract: every screen listed and each delta to today's build counted"
    state: planned
    gate: "the artifact captured in the repo; N screens and M deltas named in the ledger"
  - id: P1
    story: "tokens and primitives carry the new design (colour, type, radii, spacing)"
    state: planned
    gate: "host goldens updated; token deltas 0 of M"
  - id: P2
    story: "every screen of the artifacts looks like the artifact on the board"
    state: planned
    gate: "board-visual acked for N of N screens; deltas 0 of M"
  - id: P3
    story: "the new design costs the frame nothing"
    state: planned
    gate: "board: ux frame p99 not above the value before P1"
---

## The ask

## Measured before the cut

## Three lenses

## Not doing here

## Definition of Done
