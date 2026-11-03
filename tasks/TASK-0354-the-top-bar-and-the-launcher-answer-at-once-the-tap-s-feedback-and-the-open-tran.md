---
schema: 1
id: TASK-0354
title: "The top bar and the launcher answer at once — the tap's feedback and the open transition measured, then cut to the budget"
status: draft
owner: ["@ui"]
created: 2026-11-03
subsystem: ui
components: [desktop-shell, app-host, windowd]
kind: ui
novelty: variant
estimate: 3
goal:
  metric: "tap to the first changed frame; frames dropped while a top-bar panel or the launcher opens and while the launcher pages"
  baseline: "not measured; the operator reports both as slow (2026-11-03)"
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
    state: planned
    gate: "board cycle: tap-to-frame and frame p99 logged for the 6 panels, the launcher small and big and the pager; the dominant hop named in ms"
  - id: P1
    story: "the dominant cost P0 names is cut"
    state: planned
    gate: "board: the named hop at most half of its P0 value; tap to frame <= 100 ms"
  - id: P2
    story: "the top bar and the launcher open and page without a hitch"
    state: planned
    gate: "board: tap to frame <= 50 ms; 0 of 32 frames dropped on every open and on paging"
---

## The ask

Operator, 2026-11-03: the top bar and the launcher are very slow.

## Measured before the cut

Nothing yet: TASK-0312 and TASK-0313 proved pixel fidelity, not time. P0 measures. Suspects to price there (not yet shown):

- the Control Center's full-screen glass re-blurred on any damage (TASK-0335 context);
- the shell's CPU raster in the app-host per transition frame (TASK-0354 removes it);
- the pager glide on ScrollMomentum X.

## Three lenses

## Not doing here

- The general GPU per-pixel cost (TASK-0351).

## Definition of Done
