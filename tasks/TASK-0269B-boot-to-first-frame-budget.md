---
title: TASK-0269B Boot gates v1b — boot to FIRST FRAME is a measured budget, not a feeling
status: Draft (seeded 2026-09-20 from TASK-0077C's scope firewall)
owner: @ui @runtime @kernel
created: 2026-09-20
depends-on:
  - tasks/TASK-0269-boot-gates-v1-readiness-spawn-resource.md
follow-up-tasks: []
links:
  - Parent (Done): tasks/TASK-0269 — readiness CONTRACT, spawn failure reasons, leak sentinel.
    It proved the boot reaches its markers; it never claimed how LONG that takes.
  - Sibling budget (the interaction half): tasks/TASK-0145B-ui-interaction-latency-budget.md
  - Memory half of the same measurement: tasks/TASK-0077C-dsl-v0_2c-runtime-long-session-large-data-contract.md
  - Stage ordering this must not fight: docs/adr/0062-boot-stage-fence-and-readiness-barriers.md
  - Earlier reveal work (reveal 4.47 s → 0.65 s, and what it did NOT cover):
    memory `boot-qos-interactive-flip-learnings`, `boot-splash-reveal-open-tasks`
---

## Why a B variant rather than a new subject

TASK-0269 is Done and owns the boot *contract*: the readiness ladder, why a spawn failed, the
leak sentinel. It deliberately never claimed a TIME. This is that missing dimension on the same
subject, which is why it is 0269B and not a sixth boot ledger — the tree already carries
0178 (boot control), 0289 (boot trust floor) and 0269 (boot gates), and none of them owns speed.

## The gap (stated honestly, because it is barely measured)

The user's expectation, in their own words, is that the system loads **deterministically within
half a second**. The observed boot takes **7–11 seconds to first frame**.

What is actually measured today is one thin slice: init 1.26 s, of which the system volume is
1.19 s — stable across 14 runs. Where the remaining ~6–10 s goes, **nobody has measured**. That
is the honest starting position, and it is why this ledger's first package buys information
rather than changes.

This task must not begin with a theory. Every previous boot-speed win in this tree came from a
measurement that contradicted the obvious explanation (the reveal work, the scheduler
double-run, the display-mode race), and at least one round was lost to optimising a stage that
was not the cost.

## Goal (end system)

Time from power-on to a first frame the user would accept is a measured budget with a number,
printed in the boot log and asserted in a lane, with the cost attributed to named stages so a
regression says WHERE it happened.

## Non-goals

Interaction latency after the first frame (TASK-0145B). App-host memory (TASK-0077C). Changing
the stage-fence ordering contract (ADR-0062) — this measures inside it. Chasing TCG artefacts
as if they were product cost: the budget states its machine or it is worthless.

## Invariants

- Attribution before optimisation: no stage is touched before its share is measured.
- The budget names its machine and profile; a number without them is not a budget.
- Determinism unchanged — no "faster by being less sure it booted".
- No fake-green: `first frame` means a frame a user would accept, not a marker that precedes it.

## Packages (each ends in a gate)

- **P0** Attribution: per-stage timestamps from power-on to first frame, printed as one line
  with numbers. **Gate:** the line accounts for ≥ 95 % of the measured wall time — an
  attribution that loses seconds is not attribution.
- **P1** Budget + assert on the stages P0 says dominate. **Gate:** a lane marker with numbers
  that fails when exceeded.
- **P2+** Reduction, package per dominating stage, each justified by P0's numbers and gated by
  P1's assert moving. **Gate:** no package lands without its own before/after number.

## Definition of Done

A boot-to-first-frame number with its machine named, attributed across stages, asserted in a
lane; and if the gap to the half-second expectation remains, it is stated as a number with the
reason, not left as an impression.
