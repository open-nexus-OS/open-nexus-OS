---
title: TRACK DSL v1 DevX (SwiftUI/ArkUI/Compose-inspired): intuitive ergonomics + pro capability via bounded primitives (no “QML power”)
status: Living
owner: @ui @runtime
created: 2026-01-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Keystone map (SDK + hard apps): docs/dev/platform/keystone-map.md
  - DSL v0.1a foundations: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - DSL v0.1b interpreter + snapshots: tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
  - DSL v0.2a stores/nav/i18n core: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - DSL v0.2b svc.* stubs + demo: tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
  - DSL v0.3a AOT/codegen: tasks/TASK-0079-dsl-v0_3a-aot-codegen-incremental-assets.md
  - UI design system primitives: tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
  - Zero-copy app platform (pro workloads): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Zero-copy VMOs (keystone data plane): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
---

## Goal (track-level)

Make the DSL:

- **as intuitive as SwiftUI/ArkUI/Compose** for common app work,
- **powerful enough for “hard apps”** (Office/DAW/Studio/Video) by pairing the DSL shell with pro primitives and native widgets,
- while staying aligned with Open Nexus OS invariants:
  - determinism (goldens, stable artifacts),
  - boundedness (no unbounded work),
  - capability-first security (IO only via `svc.*` in effects/services),
  - no “QML-style unbounded scripting language” creep.

## Core stance (don’t copy, take the best)

- We adopt the **best ergonomics**:
  - component composition + props,
  - modifier-style styling,
  - environment-driven responsive branching,
  - simple state access (`$state.field`).
- We avoid the parts that create drift or non-determinism:
  - hidden global state and implicit side-effects,
  - unbounded scripting inside UI nodes,
  - stringly-typed IO/query surfaces.

## v1 readiness gates (developer-facing)

These are the “make it feel first-party” gates. Each gate must be implementable host-first with deterministic proofs.

### Gate A — Local state ergonomics (no generics required)

- `$state.field` access remains ergonomic and predictable.
- Two-way bindings are explicit and deterministic (e.g. `TextField` updates `$state.email` without hidden IO).

### Gate B — Environment injection (theme/locale/device) without magic

- `device.*` is read-only and fixture-testable.
- Theme and locale are injected deterministically (no host OS font/theme dependencies).

### Gate C — Async UI “recipes”

Provide a standard, intuitive pattern for:

- loading / error / empty / retry,
- cancellation/timeouts,
- without allowing IO inside reducers.

### Gate D — Navigation feels simple

- declarative routes + typed params,
- deep link shape is explicit,
- state restoration is bounded.

### Gate E — Large lists/tables are first-class

- virtualization is the default path for large collections,
- stable keys are required,
- paging tokens are deterministic.

### Gate F — Animation/transitions are consistent and deterministic

- small set of motion presets,
- well-defined interruption/cancel behavior,
- stable output for goldens.

### Gate G — Preview-ish iteration loop

Not necessarily live previews, but:

- fast host interpreter,
- snapshot harness,
- deterministic fixtures for profiles/locales.

### Gate H — Pro surfaces via bounded “blessed” primitives + NativeWidget

For hard apps, assume pro surfaces exist as native widgets:

- timeline canvas, waveform/meters, video preview surface, virtualized table.

The DSL remains the shell: layout, inspectors/toolbars, routing, state, effects.

## Mapping to tasks (anti-drift)

- v0.1a foundations: `TASK-0075`
- interpreter + snapshots: `TASK-0076`
- stores/nav/i18n core: `TASK-0077`
- svc.* + stubs + demo: `TASK-0078`
- AOT/codegen: `TASK-0079`

Targeted v1 DevX splits (only when needed; keep count small):

- `TASK-0077B` — v1 DevX ergonomics (local state/bindings/env/async recipes)
- `TASK-0077C` — v1 pro primitives + NativeWidget “blessed path” guidance (virtual tables/timelines)
