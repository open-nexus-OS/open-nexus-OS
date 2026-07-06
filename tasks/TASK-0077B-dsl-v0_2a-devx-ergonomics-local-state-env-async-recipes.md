---
title: TASK-0077B DSL v0.2a DevX: local `$state` (implicit instance stores) + two-way bindings + async recipes (host)
status: Draft
owner: @ui @runtime
created: 2026-01-26
updated: 2026-07-06
depends-on:
  - tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
follow-up-tasks: []
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Language reference: docs/dev/dsl/{state,syntax,patterns}.md
  - Principles this task serves: docs/dev/dsl/principles.md (encapsulation without magic)
---

## Context (updated 2026-07-06)

The v0.2a core is powerful; this task makes the common cases feel effortless —
declarative-framework ergonomics — **without hidden magic**. The masterplan pins the
mechanism: local component state compiles to **implicit per-instance stores** using the
exact same reducer machinery (no second semantics, no hidden globals); keyed identity
means local state survives collection reorders (proven in TASK-0076).

Effects support short **multi-step plans** here (the IR `EffectPlan` step list grows
beyond single-call): call → dispatch chains with explicit timeouts and cancellation.

## Goal

1. **Local state sugar**: component-level `state` field declarations lower to an
   implicit instance store + generated events for built-in mutations; `$state.field`
   read/write is the primary idiom; the store-vs-local posture documented (local =
   per-instance, ephemeral; shared/durable = named `Store`).
2. **Two-way bindings** complete + deterministic: TextField, TextArea, Checkbox,
   Toggle, Slider, Select, Stepper — bindings only update state (never IO); each is a
   dispatched built-in event through the normal reduce path.
3. **Async recipes** (documented + fixture-proven patterns, not new language):
   - loading/error/empty/retry as a canonical Store shape (`patterns.md` chapter);
   - effect **cancellation tokens**: a newer triggering event cancels the stale plan's
     pending dispatches deterministically;
   - explicit `timeoutMs` on every call step; stable error-code enums end-to-end.
4. **Environment ergonomics**: `device.*`/locale/theme reads are explicit, stable,
   fixture-injectable (no host-OS dependence anywhere).

## Non-Goals

- Generics or type-system growth (patterns.md composition instead). IO in reducers or
  views — never. Real service IPC (TASK-0078). Live-preview IDE (host snapshots are
  the loop). Kernel changes.

## Constraints / invariants (hard requirements)

- One mutation path: bindings and local-state sugar reduce through the same
  dispatch → reduce → commit pipeline (visible in IR, no runtime special cases).
- Deterministic scheduling incl. cancellation (a cancelled plan's dispatches never
  land; fixture-proven).
- Boundedness caps unchanged; zero-alloc steady state unchanged.
- **No fake success**: async recipe fixtures assert real state transitions, never a
  logged "ok" (fake-proof-marker rule).
- No `unwrap/expect`; no godfiles.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_2a_devx_host/`:

- local-state examples lower deterministically; IR shows the implicit store (golden);
- two-way binding fixtures for all seven controls update state deterministically and
  render via the narrow-invalidation path;
- local state survives keyed reorder (collection fixture);
- async recipes: loading→loaded, loading→error→retry, and cancellation (stale plan
  superseded) each with deterministic transition goldens;
- environment fixtures produce stable snapshots across profile/locale/theme variants;
- conformance corpus extended (local-state + cancellation cases).

### Docs — required (reference grade)

- `docs/dev/dsl/state.md`: local-state chapter final; `patterns.md`: async recipes
  chapter with the canonical Store shapes; `syntax.md` examples current.

## Touched paths (allowlist)

- `userspace/dsl/{core,ir,runtime}/` (extend: sugar lowering, EffectPlan steps,
  cancellation)
- `tests/dsl_v0_2a_devx_host/` (new), `tests/dsl_conformance/` (extend)
- `docs/dev/dsl/{state,syntax,patterns}.md`

## Plan (small PRs)

1. local-state lowering (implicit stores) + IR goldens
2. bindings for the seven controls + fixtures
3. EffectPlan multi-step + cancellation + async-recipe fixtures
4. docs (state/patterns/syntax)
