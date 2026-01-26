---
title: TASK-0075 DSL v0.1a: lexer/parser→AST + Scene-IR + lowering + nx dsl (fmt/lint/build) + host goldens
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI runtime baseline: tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
  - UI layout baseline: tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - UI kit baseline: tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

We want a declarative DSL for UI that targets the existing stack (runtime/layout/kit/theme), but we must start with
deterministic foundations:

- grammar + parser + AST with good diagnostics,
- a stable Scene-IR suitable for goldens,
- a lowering pass (AST → IR) with validation (keys, a11y hints),
- CLI tooling for fmt/lint/build.

Interpreter + snapshots + OS demo is deferred to v0.1b (`TASK-0076`).

## Canonical UI project layout (v0.x convention; no auto-import)

This DSL is intended to support a Nuxt-like *project structure* (without auto-import magic).
The loader and tooling must behave deterministically given the same file set.

Recommended layout (apps and SystemUI can both follow this shape):

- `ui/pages/**.nx` — top-level pages/screens
- `ui/components/**.nx` — reusable components
- `ui/composables/**.nx` — **pure** helpers and store definitions (no IO; deterministic only)
- (optional) `ui/services/**.nx` — service adapters called from **effects only** (no reducers; v0.2+)
- `ui/themes/**.nxtheme.toml` — theme/token mappings (authoring; human-editable)
  - compiled artifact (canonical, Cap'n Proto): `**.nxtheme` (optional in v0.1; see `TASK-0076`)
- `ui/platform/<profile>/**` — profile overrides (resolution rules are defined in v0.2; see `TASK-0077`)
- `ui/tests/**` — host fixtures / goldens / interaction scripts
  - keep v0.1 minimal; v0.2 introduces optional sub-layout via generators (see below)

### Layout posture: minimal by default; expanded by generators

We intentionally do **not** force a heavy scaffold (lots of empty folders/files) in v0.1.
Instead, `nx dsl` provides generators that create an expanded structure **only when needed**, keeping repos small and avoiding “fake structure”.

Recommended (optional) conventions once an app grows:

- Naming (recommended, not required):
  - `ui/composables/**.store.nx` — store definitions (state/events/reducers/effects; v0.2+)
  - `ui/services/**.service.nx` — effect-side service adapters (v0.2b+)
- Tests (optional structure):
  - `ui/tests/unit/{stores,services,composables}/`
  - `ui/tests/component/{pages,components}/`
  - `ui/tests/e2e/`
  - `ui/tests/fixtures/` + `ui/tests/goldens/`

## Goal

Deliver:

1. Workspace skeleton:
   - `userspace/dsl/nx_syntax` (lexer+parser → AST + formatter)
   - `userspace/dsl/nx_ir` (Scene-IR types + stable hashing + serializer)
   - (lowering lives in either `nx_syntax` or a dedicated crate; keep boundaries clean)
   - CLI `tools/nx-dsl` (fmt/lint/build)
2. Minimal DSL grammar (v0.1 subset):
   - Page/Component/State/Prop/Import/@computed
   - view expressions (Stack/Text/Image/Icon/Button/Card/TextField/List/Spacer)
     - optional escape hatch: `NativeWidget(handle, props)` for rare custom widgets
       - deterministic given inputs; bounded CPU/memory
       - no direct IO inside the widget; IO is only via `svc.*` in effects
       - must provide a11y semantics or be lint-rejected where required
   - modifiers (styling/layout annotations):
     - canonical form: `modifier { ... }`
     - syntactic sugar: chaining (`.padding(2).bg(accent)`) lowers to an equivalent `modifier { ... }`
     - deterministic conflict posture: duplicate setters are rejected by lint (preferred) or deterministically resolved (documented)
     - initial set (v0.1): padding/margin/size/opacity/cornerRadius/color(role)
   - bindings ($state read/write) and events (on Tap → set/emit/navigate)
3. Deterministic Scene-IR:
   - stable ordering, stable subtree hashes
   - **canonical on-disk format**: Cap'n Proto binary (`.nxir`) for determinism + bounded parsing + future OS use
   - **derived view**: stable canonical JSON (`.nxir.json`) for host goldens, diffs, and debugging
4. Host tests:
   - parse/format idempotence
   - AST→IR golden JSON stability (JSON is a view derived from canonical IR)
   - diagnostics for missing @key and missing a11y label hints

5. Module resolution (no auto-import; deterministic):
   - explicit `import "..."`
   - optional stable alias roots (e.g. `@app/...`) configured by tooling (documented in `docs/dev/dsl/cli.md`)
   - stable conflict errors (same symbol defined in two imports is an error with deterministic ordering)

### Lint posture (v0.1a)

The v0.1a lints are intentionally small but strict and deterministic:

- missing list keys (`@key`) is an error (with spans + stable diagnostic codes)
- missing a11y label hints is an error (with spans + stable diagnostic codes)
- module/import conflicts are errors with deterministic ordering

Follow-ups that are **out of scope for v0.1a** (tracked in v0.2+ tasks):

- naming conventions (Page/Component/Store/Event suffixes)
- unused symbol detection (unused state fields / unused events)
- boundedness hints (e.g. large lists without virtualization/budgets)

## Non-Goals

- Kernel changes.
- Codegen.
- Interpreter bridge / rendering / snapshots / SystemUI demo (v0.1b).
- Profile/override semantics beyond parsing (tracked in v0.2; see `TASK-0077`).

## Constraints / invariants (hard requirements)

- Deterministic formatting, hashing, and IR serialization.
- Bounded parsing:
  - cap file size,
  - cap recursion depth,
  - cap identifier lengths.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## v1 readiness gates (DevX, directional)

This task anchors the “feel” of the DSL by making the surface deterministic and easy:

- `$state.field` remains the canonical, intuitive state access idiom (bindings are explicit; IO stays out of view/reducers).
- Modifiers remain token-driven and deterministic (`modifier {}` canonical; chaining sugar lowers 1:1).
- `@when ... @else ...` is canonical; `match(expr) { ... else ... }` is sugar only and lowers 1:1.
- The formatter/linter/IR must stay stable to support a SwiftUI/ArkUI/Compose-like iteration loop (goldens, snapshots).

Track reference: `tasks/TRACK-DSL-V1-DEVX.md`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_1a_host/`:

- parse→fmt→parse is idempotent (AST equality / structural equality)
- AST→IR produces golden JSON files that are stable
- lint diagnostics include source spans and stable codes

CLI:

- `nx dsl fmt --check` exits non-zero on needed changes
- `nx dsl lint` returns non-zero on errors (warnings are reported but do not fail unless `--deny-warn`)
- `nx dsl build` emits **canonical** `.nxir` under `target/nxir/` deterministically
- `nx dsl build --emit-json` also emits `.nxir.json` under `target/nxir/` deterministically (derived view for goldens)

## Touched paths (allowlist)

- `userspace/dsl/nx_syntax/` (new)
- `userspace/dsl/nx_ir/` (new)
- `tools/nx-dsl/` (new)
- `tests/dsl_v0_1a_host/` (new)
- `docs/dev/dsl/overview.md` + `docs/dev/dsl/syntax.md` + `docs/dev/dsl/ir.md` + `docs/dev/dsl/cli.md` (new)

## Plan (small PRs)

1. repo scaffolding for syntax/ir/cli
2. lexer+parser+AST+pretty printer + diagnostics
3. IR + stable hashing + serializer
4. lowering pass + semantic lint rules (keys/a11y)
5. host tests + docs
