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
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
  - DSL v1 DevX track: tasks/TRACK-DSL-V1-DEVX.md
follow-up-tasks:
  - TASK-0077B: DevX ergonomics (local state/bindings/env/async recipes)
  - TASK-0077C: Pro primitives + NativeWidget blessed path (tables/timelines)
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

### Canonical conditional form (v0.x)

We standardize on one canonical conditional form for UI branching:

- **canonical**: `@when <cond> { ... }` with optional `@else { ... }`
- **sugar**: `match(device.profile) { ... else ... }` lowers to an equivalent `@when` chain (no new semantics)

Lint posture:

- `@when` chains are evaluated top-to-bottom; first match wins.
- For profile-driven layout branching, **missing `@else` is a lint warning by default** (upgradeable via `--deny-warn`).
  (Rationale: avoid “works on phone, broken on tv” drift.)

## Goal

Deliver:

1. Syntax/AST extensions:
   - Store, event enum, reduce blocks (pure)
   - @effect blocks triggered by event matches
   - Routes block + navigate actions
   - i18n key declarations and `@t("key")` usage
2. IR extensions:
   - IrStore / IrReducer / IrEffect / IrRoutes / IrI18n
   - stable hashing remains deterministic
   - IR serialization:
     - canonical: Cap'n Proto (`.nxir`)
     - derived: canonical JSON view (`.nxir.json`) remains deterministic for host goldens/debugging
3. Lowering validations:
   - reducers are pure (no IO, no service calls)
   - exhaustive event enums / unreachable diagnostics (where feasible)
   - unique routes, param type validation
   - `@t("key")` keys exist and are collected
4. Interpreter runtime additions:
   - store runtime:
     - **model**: state + events + reducers + effects (JS “getters/actions” naming is avoided; reducers/effects match the language semantics)
     - dispatch → reduce (pure) → commit → schedule effects (effect steps are abstract in v0.2a)
     - **boundaries**:
       - reducers are pure: no IO, no `svc.*`, no DB, no file access
       - effects may call service adapters (v0.2b) and must be bounded/time-limited
   - navigation runtime: history push/replace/back, param parsing, subtree mount/unmount
   - i18n runtime: locale packs loader + `t(key)` lookup + locale switch signal
     - authoring packs may be JSON for human editing
     - runtime prefers compiled, compact binary catalogs when available (see `TASK-0240/0241`)
   - device env runtime: fixture-backed on host; plumbed from OS later
   - markers:
     - `dsl: store runtime on`
     - `dsl: nav runtime on`
     - `dsl: i18n on`

### Lint posture (v0.2a)

Severity rule of thumb:

- **Errors**: anything that breaks determinism, correctness, or boundaries (must not ship / must not run).
- **Warnings**: quality/UX/style issues that are safe to ship but should be cleaned up; can be promoted via `--deny-warn`.

Required **errors** in v0.2a:

- reducer purity violations (reducers must be pure; reject `svc.*` / IO / side-effects in reducers)
- invalid/non-deterministic navigation (duplicate routes, invalid param types, non-deterministic resolution)
- i18n correctness (missing keys referenced by `@t("...")`)
- boundedness invariants (exceeding configured caps for history/event/effect queues)

Recommended **warnings** (v0.2a+ follow-ups):

- naming conventions (Page/Component/Store/Event suffixes for readability)
- unused events/state fields (dead logic / AI-generated leftovers)
- UX rails (missing loading/error/empty states) where applicable
- scalability hints (large lists without virtualization / missing budgets), unless a hard cap is exceeded

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
- Bounded language constructs:
  - loops are allowed, but must be **bounded** (no unbounded `while`/infinite loops in v0.x).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Session management + persistence posture (v0.2a guidance)

This task does not implement a DB, but it must keep app state architecture clean and deterministic.
Recommended tiering (applies to both interpreter and AOT):

1. **Session state (default)**:
   - in-memory store state scoped to the app instance/window/session
   - examples: selection, scroll position, in-flight request state, current route history
2. **Durable small state (typed snapshots)**:
   - persisted via a typed snapshot (`.nxs`) through the platform preferences/settings substrate
   - examples: last-opened page/doc id, user UI prefs, locale preference, pinned items
3. **Durable large/queryable state (DB)**:
   - only when real querying/indexing is required (notes content, message history, search index)
   - must be host-first and OS-gated; do not make v0.2a semantics depend on DB availability

Tooling implication:

- `nx dsl lint` may warn when reducers attempt to encode persistence/IO logic (even if syntactically allowed elsewhere),
  and should guide developers toward “effects + adapters + snapshots” instead.

## v1 readiness gates (DevX, directional)

v0.2 is where the DSL becomes app-capable. For v1 “feel”, we also require:

- Local state ergonomics and bindings remain intuitive (`$state.field`) without hidden magic (tracked in `TASK-0077B`).
- Environment (theme/locale/device) is fixture-testable and deterministic (avoid host-dependent behavior).
- Navigation is simple and deterministic (typed params, bounded history; deep link shape is explicit).
- Large data surfaces do not require DSL “power language”: pro primitives + NativeWidget are the path (tracked in `TASK-0077C`).

Track reference: `tasks/TRACK-DSL-V1-DEVX.md`.

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
- `docs/dev/dsl/state.md` + `docs/dev/dsl/navigation.md` + `docs/dev/dsl/i18n.md` (new/extend)
- `docs/dev/dsl/profiles.md` (new: device env + override resolution rules)

## Plan (small PRs)

1. grammar/AST extensions + formatter updates
2. IR nodes + stable hashing/serializer updates
3. lowering validations and diagnostics
4. interpreter store/nav/i18n runtimes + markers
5. host tests + docs
