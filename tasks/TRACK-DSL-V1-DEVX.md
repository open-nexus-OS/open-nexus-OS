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

## App-driven capability expansion map

The DSL must grow by shipping real apps and system surfaces, not by adding abstract language features in isolation.
Whenever a task builds a user-facing app/surface, it should explicitly state which DSL capability it strengthens.

### Pure DSL document/app surfaces

- `TASK-0093` Markdown viewer:
  - document/article layout
  - mixed block rendering (headings/lists/code/links/images)
  - find highlight presentation
- `TASK-0140` Updates settings page:
  - long-running task/status surfaces
  - picker-driven offline workflow UI
- `TASK-0121` / `TASK-0122` Settings + Notifications:
  - large settings forms
  - sidebar/detail settings navigation
  - notification center/list grouping
- `TASK-0118` Accessibility settings:
  - accessibility-heavy settings forms and previews
- `TASK-0175` Language & Region:
  - locale-sensitive preview surfaces inside settings
- `TASK-0086` Files:
  - large virtualized lists/grids
  - selection mode
  - empty/loading/error/productivity shell patterns

### DSL shell + blessed native surface

- `TASK-0092` PDF viewer:
  - PDF chrome/search/export in DSL
  - blessed document canvas / page viewport surface
- `TASK-0098` RichText + Notes:
  - DSL shell + editor chrome
  - blessed rich text editing primitive
- `TASK-0106` Camera/Gallery:
  - camera chrome in DSL
  - blessed media preview surface
  - gallery timeline/grid patterns
- `TASK-0111` WebView:
  - browser/web container chrome in DSL
  - blessed embedded offscreen web surface
- `TASK-0117` Captions / Video:
  - caption controls and media chrome in DSL
  - timed media/caption overlay on blessed video surface
- `TASK-0100B` Audio Mixer:
  - mixer shell in DSL
  - bounded meters/waveforms as blessed media widgets if needed later

### System surfaces that must also converge to DSL

- `TASK-0083` document picker overlay
- `TASK-0088` print preview/dialog
- `TASK-0105` capture overlay
- `TASK-0096` IME candidate UI + OSK shell
- `TASK-0069` notifications actions/reply UI
- `TASK-0125` heads-up / lock redaction / notification settings surfaces
- `TASK-0071` command palette/search overlay
- `TASK-0151` Search v2 palette surface
- `TASK-0127` share chooser
- `TASK-0126D` chat action cards
- `TASK-0233` SAF picker flows / remember-access UX
- `TASK-0257` battery indicator/detail sheet
- `TASK-0187` WebView file chooser reuse path
- `TASK-0206` browser history/session/CSP viewer surfaces

### Session/security shell states

- `TASK-0109` lockscreen
- `TASK-0110` OOBE / Greeter / Accounts

These are still DSL-first at the visible shell layer, but their authority remains in dedicated services (`lockd`,
`identityd`, `keymintd`, etc.). UI must not fake or replace the security boundary.

## Blessed primitives / embedded surfaces to standardize

To stay "as intuitive as SwiftUI/ArkUI/Compose" while supporting real apps, we should standardize a **small** set of
bounded blessed primitives instead of inventing ad-hoc custom views in each task:

- document canvas / page viewport
- rich text editor surface
- media preview surface (camera/video/image)
- embedded offscreen web surface
- large virtualized table/grid/timeline surfaces
- meter/waveform/timeline surfaces for pro/media apps

Rule:

- the DSL remains the shell for state, layout, routing, toolbars, inspectors, dialogs, and effects
- blessed primitives host the specialized rendering/input core only
- any new primitive must be justified by at least one concrete task/app and documented back into this track

Targeted v1 DevX splits (only when needed; keep count small):

- `TASK-0077B` — v1 DevX ergonomics (local state/bindings/env/async recipes)
- `TASK-0077C` — v1 pro primitives + NativeWidget “blessed path” guidance (virtual tables/timelines)
