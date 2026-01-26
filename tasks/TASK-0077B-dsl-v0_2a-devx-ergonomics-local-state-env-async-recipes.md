---
title: TASK-0077B DSL v0.2a DevX: local state ergonomics (`$state.field`) + bindings + environment + async UI recipes (deterministic)
status: Draft
owner: @ui
created: 2026-01-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v0.2a core: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - DSL v0.2b stubs/CLI: tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
  - DSL v1 DevX track: tasks/TRACK-DSL-V1-DEVX.md
  - DSL patterns: docs/dev/dsl/patterns.md
  - Security standards: docs/standards/SECURITY_STANDARDS.md
---

## Context

The v0.2 core (stores/effects/navigation/i18n) is powerful, but DevX can still feel “frameworky” unless the common
cases are extremely easy and intuitive—without adding hidden magic.

This task nails the **SwiftUI/ArkUI/Compose-like ergonomics** while preserving determinism and boundedness.

## Goal

Deliver a clear, deterministic, dev-friendly surface for:

1. **Local state ergonomics**:
   - `$state.field` read/write is the primary idiom in DSL pages/components.
   - The “store vs local state” posture is explicit and documented (avoid hidden globals).
2. **Bindings**:
   - two-way bindings for common primitives (TextField, Checkbox, Switch, Slider) are deterministic.
   - bindings are not “magical IO”: they only update state; IO stays in effects/services.
3. **Environment access**:
   - `device.*` remains read-only and fixture-testable,
   - locale/theme are injected deterministically (no host OS dependence),
   - environment reads are stable and explicit.
4. **Async UI recipes** (standard patterns):
   - loading/error/empty/retry guidance that every app can follow,
   - cancellation + timeouts are explicit (effects/services only),
   - stable error codes (no stringly errors).

## Non-Goals

- Adding full generics or a complex type system.
- Allowing IO in reducers or view nodes.
- Building a “live preview IDE”; we rely on host interpreter + snapshots for now.

## Constraints / invariants (hard requirements)

- Deterministic state update ordering (no hidden scheduling).
- Boundedness: caps on event/effect queues and list sizes still apply.
- No fake success: async recipes must not log “ok” without real behavior.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_2a_devx_host/` (or equivalent):

- `$state.field` binding examples compile and lower deterministically.
- Two-way binding fixtures update state deterministically (TextField/Checkbox/Switch/Slider).
- Environment fixtures (profile/locale/theme) produce stable snapshots.
- Async recipe examples demonstrate stable loading/error/empty rendering with deterministic transitions.

## Touched paths (allowlist)

- `userspace/dsl/nx_syntax/` (surface sugar as needed)
- `userspace/dsl/nx_ir/` (binding/env nodes if needed)
- `userspace/dsl/nx_interp/` (binding semantics)
- `tests/dsl_v0_2a_devx_host/` (new)
- `docs/dev/dsl/state.md` + `docs/dev/dsl/syntax.md` (extend: guidance + examples)
