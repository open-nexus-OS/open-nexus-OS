---
title: TASK-0145B Perf v1c-B: UI interaction latency has a budget with a NUMBER, like the kernel paths already do
status: Draft (seeded 2026-09-20 from TASK-0077C's measurement — the scope firewall for it)
owner: @ui @runtime
created: 2026-09-20
depends-on:
  - tasks/TASK-0145-perf-v1c-deterministic-gates-scenes.md
  - tasks/TASK-0077C-dsl-v0_2c-runtime-long-session-large-data-contract.md
follow-up-tasks: []
links:
  - Memory sibling (the other half of the same measurement): tasks/TASK-0077C-dsl-v0_2c-runtime-long-session-large-data-contract.md
  - Done predecessor that claimed the latency subject: tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md
  - Parent (Draft): tasks/TASK-0145-perf-v1c-deterministic-gates-scenes.md — "deterministic perf
    gates for key scenes + baseline artifacts + OS markers". This B variant is that subject cut
    down to the one budget a measurement now demands, so the parent stops being a five-year plan.
  - Rest of the perf family, also Draft — USE or RETIRE, never add a sixth:
      tasks/TASK-0143 (perfd tracer), tasks/TASK-0144 (pacing hooks + HUD + nx perf),
      tasks/TASK-0172 (sessions API), tasks/TASK-0173 (scenarios + budgets + gates + CI reports)
  - The pattern to copy (a budget with numbers, asserted): `KSELFTEST: ipc call budget ok (rt=<n>us …)`,
    tasks/TASK-0054C-ui-v1a-kernel-ipc-fastpath-control-plane-vmo-bulk.md
  - Budget constants precedent: source/kernel/neuron/src/core/trap/budgets.rs
---

## Why a B variant in the 1xx UI range

UI work is numbered below 200 in this tree, and the perf subject already lives there as five
Draft ledgers. Adding a fresh number would have been a sixth claim on one subject. This is
TASK-0145's own subject — *deterministic perf gates for key scenes* — recut to the single
budget a measurement now demands, so something lands instead of being planned again.

## Why this exists (2026-09-20)

While measuring TASK-0077C's memory ceiling, the asymmetry became impossible to ignore: the
kernel and IPC paths assert budgets **with numbers** in the boot log — `KSELFTEST: ipc call
budget ok (rt=<n>us handoff_miss=<m> alloc=0)`, `KSELFTEST: runtime timer budget ok`,
`runtime ipi budget ok` — and the UI path asserts **nothing**. `docs/dev/dsl/perf.md` is a stub
that says where the cost "should" be and contains not one measured number.

So the system can prove an IPC round trip stays inside its budget, and cannot prove that
touching a control produces a frame inside any budget at all. That is the gap between "it
works" and a product.

This ledger exists as much to be a FENCE as to be work: TASK-0077C is a memory task, and every
latency question found while building it belongs here instead of there.

## Ground truth (verified 2026-09-20, do not re-assume)

- **No UI latency instrumentation exists.** No `apphost:` timing marker, no render-span line;
  `grep` for a frame/latency budget in app-host and windowd finds only the word, never a number.
- **The perf family is five Draft ledgers for one subject** (0143/0144/0145/0172/0173).
  0145 is literally *"deterministic perf gates for key scenes + baseline artifacts + OS markers"*.
  This task must RECUT or CONSUME them, never become a sixth.
- **Known numbers, all from history rather than a gate** (`memory: perf-doc-stale-real-numbers`):
  3–4 ms present median, 26–44 Hz loop, measured under TCG with host virgl. `perf.md` still
  claims "2 FPS / 468 ms" elsewhere in the tree. Nothing re-measures either on a schedule.
- **"Zero-alloc steady scroll" is claimed in TASK-0077C's invariants and is NOT proven**:
  `zero_alloc` exists in the tree only for blur (`userspace/ui/effects/src/blur.rs`). A claim
  with no gate is exactly the class this project has spent the last week deleting.

## Input from Block 1 (2026-10-09) — the cost of a structural change (TASK-0251 finding 8)

Measured with a counting allocator over the REAL apps
(`tests/dsl_apps_conformance/tests/frame_arena_budget.rs`, a recorded budget per app): the chat's
first page (60 of 240 rows through QuerySpec — the lazy loading works) costs ONE frame 270 KB of
scene and 419 KB of layout (412 boxes, 135 text runs), the same at 960x620 and 1440x814. The
cause is the pretext contract half wired (RFC-0057): text widths come from baked advances, but
app-host builds a fresh `LayoutEngine` per layout and lays out every box, `nexus-shape`'s
prepared-text caches (paragraph + line layout) are not connected, and nothing re-lays out only
the changed subtree — the scroll band makes scrolling free and every structural change pay the
whole band. The reduction this ledger's P2+ owns once P0 attributes it: persistent prepared
text across frames and incremental relayout of the changed subtree (a resize or a new row
re-evaluates, it does not re-measure — the operator's standing requirement for layout). The
budget test above is the regression signal; the frame arena's size is the memory lane's
(M2/M5, TASK-0290 "Input from Block 1"), not this ledger's.

## Goal (end system)

Touching a control produces a frame inside a budget that is a NUMBER, measured on a
deterministic scene, printed in the boot log, and asserted — so a regression fails a lane
instead of being noticed by a person months later.

## Non-goals

Memory flatness (TASK-0077C owns it). Boot time to first frame (TASK-0269B owns it). A perf
HUD, a tracer UI, or a CLI — those are 0143/0144/0172 and are not required for a budget to
exist. GPU/present-path optimisation beyond what the budget proves necessary.

## Invariants

- The budget is a constant with provenance (how it was measured, on what scene, under which
  profile) — never a round number someone liked.
- Measured on a FIXED scene and input script, so two runs are comparable.
- Asserted in a lane, with the numbers in the marker. No bare `ok`.
- TCG is not the target machine: the budget says which multiplier it assumes, or it is measured
  on the path the user actually sees.

## Packages (each ends in a gate, or it is not a package)

- **P0** Recut: absorb the parent's scope that this budget needs, and retire by decision
  whatever of 0143/0144/0172/0173 does not survive review. Blast: paper. **Gate:** no two
  ledgers claim the same subject afterwards.
- **P1** Measure: interaction → frame on a fixed scene, host-first, with the numbers written
  down and their provenance. **Gate:** a host test that prints the number; no assert yet.
- **P2** Budget + marker: the constant, `apphost: interaction budget ok (…us …)` in the visible
  lane, registered in the proof manifest. **Gate:** the marker asserts and fails when exceeded.
- **P3** Subtree-scoped re-emit — MOVED HERE from TASK-0077C D3/P4 on 2026-09-20. Its ledger
  justification there was memory, and once the emit-generation arena lands, churn is free; what
  subtree re-emit actually buys is less work per interaction, i.e. this budget. **Gate:** the
  emit-counter fixture (one row changes ⇒ exactly one subtree re-emitted) AND a measured
  improvement against P2's number.
- **P3b** OS structural-interaction lane — handed over from TASK-0077C on 2026-09-21. The
  click storm exists (`tools/qmp_click_storm.py`: open/close the Control Center over QMP,
  two structural frames per pair, verdict read off `apphost: frame arena (… base=…)`), but
  it cannot run today: QEMU accepts ONE QMP client and the visible profile's own injector
  holds it from t+120 s to the end of the ladder (measured, five attempts), and without
  `QEMU_INPUT_AUTOINJECT` there is no socket at all. This budget needs a driver anyway, so
  the lane lands here: a profile whose injector IS the storm (or hands the socket over),
  asserting `base` unchanged across ≥ 200 structural interactions. **Gate:** the lane runs
  in `test-all` and fails on a moving base.
- **P4** The scroll claim: either a zero-alloc-steady-scroll gate, or the sentence comes out of
  TASK-0077C's invariants. **Gate:** whichever it is, no unproven claim survives this package.

## Definition of Done

A budget constant with written provenance; a lane marker carrying the numbers and failing when
exceeded; `perf.md` holding measured numbers instead of intentions; the perf-family Drafts
resolved; and no claim about UI performance left in the tree without a gate behind it.
