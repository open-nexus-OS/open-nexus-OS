---
schema: 1
id: TASK-0359
title: "The platform draws what the design artifacts ask: text wraps, a border can be one side, the keyboard band follows the keyboard, emoji in colour"
status: draft
owner: ["@ui"]
created: 2026-11-10
updated: 2026-11-10
subsystem: ui
kind: ui
novelty: variant
estimate: 3
goal:
  metric: "design deltas and transitions open because the platform cannot draw or place them"
  baseline: "21 deltas and 2 transitions counted 2026-11-10 (moved from TASK-0353 P3/P5): wrapped text, one-sided borders, the keyboard band, colour emoji, gradients, the focus outline, edge fades, adaptive grids, container-relative lengths, corner anchors, wrapping rows"
  target: "0 of those open"
  instrument: "the TASK-0353 audit tables re-run against the spec files"
consumer: TASK-0353/P3
phases:
  - id: P0
    story: "text wraps: the baked measurer breaks a run at its width into lines (at spaces, CJK at characters), a modifier clamps to n lines with an ellipsis, and the glyph pass draws each line — chat bubbles, notification bodies and clipboard cards wrap"
    state: planned
    gate: "host: measure tests (three words at a two-word width give two lines; a clamp to one line ends in an ellipsis), a chat bubble golden; board-visual"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): osk 41"
  - id: P1
    story: "a border can be one side: the DSL draws a top or a bottom edge alone (the keyboard tray's lit top edge, a bar's hairline), the painter strokes each side it is given"
    state: planned
    gate: "host: painter test per side, modifiers.md, every docs example compiles"
  - id: P2
    story: "the keyboard band follows the keyboard: windowd sizes the on-screen keyboard's band to the keyboard's height, slides it in on the design's glide, leaves the ground outside its rounded corners transparent, and a key dips to 94 % while pressed"
    state: planned
    gate: "QEMU: the band's height equals the keyboard's; board-visual: the keyboard slides in and its corners show the desktop"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): osk 51 52 53 54; P5 transitions M18 M77"
  - id: P3
    story: "emoji in colour: the emoji panel and its category tabs draw colour glyphs baked from an open emoji set"
    state: planned
    gate: "host: the atlas holds the panel's emoji in colour (a golden); board-visual"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): osk 6 26 45 46"
  - id: P4
    story: "the painter draws what the artifacts' surfaces ask: horizontal and diagonal gradients (the shimmer band, the appearance mocks, the battery gauge), a focus outline in the control's own shape on every control, a fade over a region's edge (the panels' 28 px bottom fade, the lock screen's vignette over its lower 30 %)"
    state: planned
    gate: "host: painter tests and goldens per feature; board-visual"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): primitives-core SKL-4; shell-bars PL-04; shell-panels PN-GAUGE; shell-panels PN-FADE; launcher-lock L27; launcher-lock L50; settings-stash S46"
  - id: P5
    story: "the layout places what the artifacts ask: adaptive grid tracks (minmax(min, 1fr)), a length relative to the container (a modal's maximum height), a child anchored to its parent's corner (a badge on an icon), rows and columns that wrap (the desktop's icon field)"
    state: planned
    gate: "host: layout tests per rule, modifiers.md, goldens"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): primitives-window GR-1; primitives-window MD-9; primitives-window AB-8; shell-panels PN-BADGE; launcher-lock I1"
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
