---
schema: 1
id: TASK-0353
title: "The desktop matches the design of the Claude artifacts: tokens, primitives and screens, without a frame-budget regression"
status: draft
owner: ["@ui"]
created: 2026-11-03
updated: 2026-11-03
subsystem: ui
components: [desktop-shell, app-host, windowd, window-kit, widgets]
kind: ui
novelty: variant
estimate: 3.5
goal:
  metric: "screens of the artifacts that match on the board; design deltas left (tokens, sizes, radii, spacing, motion); frames dropped per transition; dead folders left in userspace/apps and userspace/ui/widgets"
  baseline: "0 screens matched; deltas counted in P0; reference numbers set, not researched; demo-exit0 and launcher still in userspace/apps, widgets/window/chrome without a known user since TASK-0337"
  target: "every artifact screen matched (fixed sizes and radii exact, padding/gap on the 4px grid, as TASK-0312/0313); every artifact transition on a motion token; 0 deltas left; 0 of 32 frames dropped per transition; frame p99 not above today's; every reference number with >= 2 sources; 0 references left to the removed folders"
  instrument: "board-visual ack per screen · host goldens · ux frame p99 line · reference note with sources · repo-wide reference search · just test-all"
consumer: LU.1
depends_on: [TASK-0354, TASK-0355]
links:
  - label: Source
    target: "Claude artifacts from the operator (2026-11-03), captured into the repo in P0"
  - label: Pattern
    target: "TASK-0312, TASK-0313 (pixel-fidelity contract of the earlier handoffs)"
  - label: Budgets
    target: "RFC-0108 (the numbers P1 confirms or replaces)"
phases:
  - id: P0
    story: "the artifacts are the design contract: every screen and every transition listed and each delta to today's build counted"
    state: planned
    gate: "the artifact captured in the repo; N screens, T transitions and M deltas named in the ledger"
  - id: P1
    story: "the reference numbers the gates use are researched on the web (a session with web access, e.g. Claude Code): the MacBook Neo's display rate, its click-to-photon latency, the system transitions' durations and springs"
    state: planned
    gate: "a reference note with >= 2 sources per number; the gates of TASK-0351, TASK-0354 and TASK-0355 set from it (today: tap <= 50 ms, 0 of 32 dropped, p99 <= 16.7 ms)"
  - id: P2
    story: "tokens and primitives carry the new design (colour, type, radii, spacing)"
    state: planned
    gate: "host goldens updated; token deltas 0 of M"
  - id: P3
    story: "every screen of the artifacts looks like the artifact on the board"
    state: planned
    gate: "board-visual acked for N of N screens; deltas 0 of M"
  - id: P4
    story: "the new design costs the frame nothing"
    state: planned
    gate: "board: ux frame p99 not above the value before P2"
  - id: P5
    story: "the artifacts' motion is the system's motion: every transition they show runs on the compositor's springs (TASK-0354)"
    state: planned
    gate: "T of T transitions mapped to a token, host goldens sampled at named times; board: 0 of 32 frames dropped per transition"
  - id: P6
    story: "userspace/apps and userspace/ui/widgets hold only what ships: demo-exit0 and launcher removed, widgets/window/chrome removed if nothing uses it"
    state: planned
    gate: "0 references to demo-exit0, apps/launcher and window::chrome in code, manifests, product.toml, markers and qemu-test.sh (or chrome's user named and kept); just test-all green"
---

## The ask

## Measured before the cut

## Three lenses

## Not doing here

## Definition of Done
