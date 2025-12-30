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
- `ui/composables/**.nx` — stores/reducers/effects helpers (no IO; pure helpers only)
- `ui/themes/**.nxtheme` — theme/token mappings (read-only in v0.1)
- `ui/platform/<profile>/**` — profile overrides (resolution rules are defined in v0.2; see `TASK-0077`)
- `ui/tests/**` — host fixtures / goldens / interaction scripts (format is tracked separately)

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
   - modifiers (padding/margin/size/opacity/cornerRadius/color(role))
   - bindings ($state read/write) and events (on Tap → set/emit/navigate)
3. Deterministic Scene-IR:
   - stable ordering, stable subtree hashes, stable JSON serializer
4. Host tests:
   - parse/format idempotence
   - AST→IR golden JSON stability
   - diagnostics for missing @key and missing a11y label hints

5. Module resolution (no auto-import; deterministic):
   - explicit `import "..."`
   - optional stable alias roots (e.g. `@app/...`) configured by tooling (documented in `docs/dsl/cli.md`)
   - stable conflict errors (same symbol defined in two imports is an error with deterministic ordering)

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

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_1a_host/`:

- parse→fmt→parse is idempotent (AST equality / structural equality)
- AST→IR produces golden JSON files that are stable
- lint diagnostics include source spans and stable codes

CLI:

- `nx dsl fmt --check` exits non-zero on needed changes
- `nx dsl lint` returns non-zero on errors (warnings are reported but do not fail unless `--deny-warn`)
- `nx dsl build` emits `.nxir.json` under `target/nxir/` deterministically

## Touched paths (allowlist)

- `userspace/dsl/nx_syntax/` (new)
- `userspace/dsl/nx_ir/` (new)
- `tools/nx-dsl/` (new)
- `tests/dsl_v0_1a_host/` (new)
- `docs/dsl/overview.md` + `docs/dsl/syntax.md` + `docs/dsl/ir.md` + `docs/dsl/cli.md` (new)

## Plan (small PRs)

1. repo scaffolding for syntax/ir/cli
2. lexer+parser+AST+pretty printer + diagnostics
3. IR + stable hashing + serializer
4. lowering pass + semantic lint rules (keys/a11y)
5. host tests + docs
