---
title: TASK-0077 DSL v0.2a: stores/reducers/effects + routes/navigation + i18n keys (syntax/IR/lowering + interp runtimes)
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v0.1a foundations: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - DSL v0.1b interpreter baseline: tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
  - UI runtime baseline: tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
  - UI layout baseline: tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - UI kit baseline: tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
---

## Context

DSL v0.2 adds “real app mechanics” on top of v0.1:

- state management (stores/reducers/events),
- deterministic effects scheduling,
- navigation/routes with params/history,
- i18n key collection and locale switching.

This task (v0.2a) focuses on language + IR + interpreter runtime foundations. Service-call stubs and the
master-detail demo app are handled in v0.2b (`TASK-0078`).

## Device/profile environment contract (v0.2a)

To support “one DSL across phone/tablet/desktop/TV/auto/foldable” deterministically, the runtime exposes a small,
read-only device environment. Host tests provide fixtures; OS wiring provides real values later.

Expose (read-only):

- `device.profile`: enum `{ phone, tablet, desktop, tv, auto, foldable }`
- `device.posture`: enum `{ flat, half_fold, tent, book }` (optional; only valid when `profile==foldable`)
- `device.sizeClass`: enum `{ compact, regular, wide }`
- `device.dpiClass`: enum `{ low, normal, high }`
- `device.input`: flags `{ touch, mouse, kbd, remote, rotary }`

## Profile overrides (path-based; no auto-import)

Tooling may support *deterministic* profile overrides via a fixed resolution order (no globbing at runtime):

- If present, `ui/platform/<profile>/pages/<Page>.nx` overrides `ui/pages/<Page>.nx`
- If present, `ui/platform/<profile>/components/<Comp>.nx` overrides `ui/components/<Comp>.nx`
- Otherwise, fall back to the base file.

Override resolution must be:

- deterministic (fixed precedence; no filesystem iteration order dependence),
- explicit (missing override falls back cleanly),
- linted (ambiguity/conflicts are errors).

Inline branching (`@when device.profile==... { ... }`) is allowed, but must lower to deterministic IR with bounded
branch evaluation (no hidden time-based switching).

## Goal

Deliver:

1. Syntax/AST extensions:
   - Store, event enum, reduce blocks (pure)
   - @effect blocks triggered by event matches
   - Routes block + navigate actions
   - i18n key declarations and `@t("key")` usage
2. IR extensions:
   - IrStore / IrReducer / IrEffect / IrRoutes / IrI18n
   - stable hashing and JSON serialization remain deterministic
3. Lowering validations:
   - reducers are pure (no IO, no service calls)
   - exhaustive event enums / unreachable diagnostics (where feasible)
   - unique routes, param type validation
   - `@t("key")` keys exist and are collected
4. Interpreter runtime additions:
   - store runtime: dispatch → reduce → schedule effects (effect steps are abstract in v0.2a)
   - navigation runtime: history push/replace/back, param parsing, subtree mount/unmount
   - i18n runtime: locale packs loader + `t(key)` lookup + locale switch signal
   - device env runtime: fixture-backed on host; plumbed from OS later
   - markers:
     - `dsl: store runtime on`
     - `dsl: nav runtime on`
     - `dsl: i18n on`

## Non-Goals

- Kernel changes.
- IDL service call stubs and effect “svc.*” calls (v0.2b).
- Full-blown language semantics (exceptions/try/catch syntax can be stubbed or rejected in v0.2a).

## Constraints / invariants (hard requirements)

- Deterministic reducer behavior and update ordering.
- Side-effects must not run inside reducers; effects are scheduled after state commit.
- Bounded runtime:
  - cap queued events per frame,
  - cap effect queue length,
  - cap route history length.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_2a_host/`:

- reducer purity lint: reducers that attempt “svc.*” are rejected
- store runtime: dispatch event updates state deterministically
- navigation runtime: route push/params parse/replace/back behavior deterministic
- i18n: required_keys extracted from IR; locale switch updates `t(key)` values deterministically (host fixture packs)
- device env: `device.profile` fixture changes branch selection deterministically (no layout/IR drift beyond the intended variant)

## Touched paths (allowlist)

- `userspace/dsl/nx_syntax/` (extend)
- `userspace/dsl/nx_ir/` (extend)
- `userspace/dsl/nx_interp/` (extend: store/nav/i18n runtimes)
- `userspace/dsl/nx_env/` (new or in `nx_interp`: device env types + host fixtures)
- `tests/dsl_v0_2a_host/` (new)
- `docs/dsl/state.md` + `docs/dsl/navigation.md` + `docs/dsl/i18n.md` (new/extend)
- `docs/dsl/profiles.md` (new: device env + override resolution rules)

## Plan (small PRs)

1. grammar/AST extensions + formatter updates
2. IR nodes + stable hashing/serializer updates
3. lowering validations and diagnostics
4. interpreter store/nav/i18n runtimes + markers
5. host tests + docs
