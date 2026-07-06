---
title: TASK-0077C DSL v0.2c Pro UI: virtualized list/table/grid + timeline contracts + NativeWidget blessed path (demand-gated)
status: Draft
owner: @ui @runtime
created: 2026-01-26
updated: 2026-07-06
depends-on:
  - tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
follow-up-tasks: []
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md (blessed-primitive set + "no scripting creep")
  - NativeWidget contract: docs/dev/dsl/syntax.md
  - Existing virtualization to promote: userspace/ui/widgets/virtual-list (nexus-virtual-list,
    production, windowd-consumed) — promote the BEST impl, don't rebuild
  - Widget home: userspace/ui/widgets/* (NOTE: `userspace/ui/kit/` does not exist — stale ref removed)
  - Zero-copy data plane gate: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QuerySpec paging feeding these surfaces: tasks/TASK-0078B
---

## Context (updated 2026-07-06)

Hard apps (office/BI, audio workstation, video editing) need a few "pro surfaces" that
must not turn the DSL into an unbounded scripting language. The answer is (a) a small
set of virtualized/timeline **contracts** rendered by first-party widgets, and (b) one
**blessed NativeWidget path** for heavy canvases — same determinism/boundedness rules
on every tier.

**Sequencing (masterplan):** the **virtualized `List` core is pulled forward into the
Phase-4 wave** (TASK-0077 consumer contract) because the master-detail demo (0078) and
the launcher grid (0080B) need it. The rest of this task — table/grid, timelines,
NativeWidget hosting — is **demand-gated**: it lands when the first real consumer app
task (0092 PDF / 0098 rich text / 0100B mixer / office wave) pulls it, per the track's
"app-driven capability expansion" rule. Do not build speculative primitives.

**IST corrections:** virtualization already exists in production
(`nexus-virtual-list`, used by windowd) — the DSL windowed-ForEach must promote/wrap
it, not re-implement scroll physics. `userspace/ui/kit/` never existed; widgets live in
`userspace/ui/widgets/*`.

## Goal

1. **Virtualized list (pulled into Phase 4)**: windowed keyed ForEach backed by
   `nexus-virtual-list` physics; stable keys, bounded live instances, deterministic
   window mapping; QuerySpec page-token hook (backpressure: at most one page fetch in
   flight).
2. **Table/grid contract** (demand-gated): column model as data, bounded rows/columns
   per viewport, deterministic ordering + selection semantics, paging tokens.
3. **Timeline contract** (demand-gated): tracks/clips/keyframes as data; deterministic
   zoom/scroll mapping; bounded rendering; selection semantics shared across consumers.
4. **NativeWidget hosting** (demand-gated): registry of capability-gated handles
   (registered at build, no dynamic loading); the runtime hosts the widget as a leaf
   with the standard invalidation contract; strict rules — deterministic given inputs,
   bounded CPU/memory per frame, no direct IO (svc.* via effects only), a11y required;
   host golden strategy (scripted input replay → frame-sequence goldens).

## Non-Goals

- Any scripting/expression growth in the DSL for these surfaces (data + contracts
  only). Building actual office/DAW/video apps. Kernel changes.

## Constraints / invariants (hard requirements)

- Virtualization mandatory for large collections (lint: unbounded non-virtualized
  collection over budget = error).
- Deterministic input replay for pro widgets (bounded event streams, no wall-clock).
- Explicit cache budgets/eviction for every pro surface; zero-alloc steady scroll.
- Promote-best rule: wrap `nexus-virtual-list`; no parallel scroll/window
  implementation.
- No `unwrap/expect`; no godfiles.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_pro_primitives_host/`:

- windowed list: deterministic window mapping fixtures (scroll position → live
  instance set), reorder/insert stability, page-fetch backpressure fixture
  (Phase-4 portion);
- table/grid + timeline: deterministic goldens for fixtures + scripted interaction
  frame sequences (when demand-gated portion lands);
- NativeWidget: budget-enforcing harness proves bounded frame cost; determinism via
  input-replay goldens (when it lands).

### Docs — required (reference grade)

- `docs/dev/dsl/patterns.md`: large-data chapter (virtualization + paging);
- NativeWidget blessed-path guidance in `docs/dev/dsl/syntax.md` +
  per-contract docs under `docs/dev/ui/` as portions land.

## Touched paths (allowlist)

- `userspace/ui/widgets/*` (table/grid/timeline widget crates when pulled)
- `userspace/dsl/runtime/` (windowed ForEach, NativeWidget host leaf)
- `tests/ui_pro_primitives_host/` (new)
- `docs/dev/dsl/{syntax,patterns}.md`, `docs/dev/ui/`

## Plan (small PRs)

1. windowed List core (rides with Phase 4 / TASK-0077)
2. [demand-gated] table/grid contract + widget + goldens
3. [demand-gated] timeline contract + widget + goldens
4. [demand-gated] NativeWidget registry + hosting + replay harness + docs
