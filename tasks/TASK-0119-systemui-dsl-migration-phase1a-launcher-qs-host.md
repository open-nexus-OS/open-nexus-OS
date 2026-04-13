---
title: TASK-0119 SystemUI→DSL Migration Phase 1a: Launcher + Quick Settings DSL pages + bridge + host snapshots/interactions
status: Draft
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Bootstrap shell + early launcher host phase: tasks/TASK-0080B-systemui-dsl-bootstrap-shell-launcher-host.md
  - Bootstrap shell visible OS mount: tasks/TASK-0080C-systemui-dsl-bootstrap-shell-os-wiring.md
  - DSL v0.1 interpreter: tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
  - DSL v0.2 app mechanics (optional): tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - UI layout pipeline contract: docs/dev/ui/foundations/layout/layout-pipeline.md
  - Design kit (primitives): tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
  - SystemUI overlays baseline: tasks/TASK-0070-ui-v8b-wm-resize-move-shortcuts-settings-overlays.md
  - Search/prefs/media dependencies (bridged): tasks/TASK-0071-ui-v9a-searchd-command-palette.md
  - Prefs store: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Media sessions: tasks/TASK-0101-ui-v16c-media-sessions-systemui-controls.md
  - Audio mixer: tasks/TASK-0100-ui-v16b-audiod-mixer.md
---

## Context

We are starting the SystemUI → DSL migration. Phase 1 targets:

- Launcher page
- Quick Settings overlay

Launcher-first note:

- the earliest visible launcher/bootstrap shell may already exist via `TASK-0080B`/`TASK-0080C`
- this phase should **upgrade and converge** that launcher into the canonical DSL Launcher page, not replace it with a second launcher implementation

Shared workspace note:

- `TASK-0119` establishes the canonical `userspace/systemui/dsl/` workspace that later phases reuse
- Phase 1 owns the shell root (`Launcher`, `QuickSettings`, shared `components/`, shared `composables/`, shared `services/`, shared `themes/`)
- later Settings/Notifications pages and specialized Settings surfaces must extend this same workspace rather than creating parallel DSL roots

This phase must preserve behavior/visual parity and accessibility labels, and be proven via deterministic host tests.

OS wiring and postflight markers are handled in Phase 1b (`TASK-0120`).

## Goal

Deliver:

1. SystemUI DSL workspace:
   - `userspace/systemui/dsl/pages/Launcher.nx`
   - `userspace/systemui/dsl/pages/QuickSettings.nx`
   - shell root / shared navigation state for the visible SystemUI DSL shell
   - reusable DSL components under `components/`
   - pure store helpers under `composables/`
   - effect-side adapters under `services/`
   - `themes/` mapping to existing tokens (light/dark/HC)
   - page files follow the canonical `Store` + `Event` + `reduce` + `@effect` + `Page` shape from `TASK-0075`
2. `userspace/systemui/dsl_bridge` crate:
   - safe adapters to system services used by the DSL pages:
     - app list + launch (`appmgrd`)
     - quick settings state and toggles (`prefsd`)
     - volume slider (`audiod`)
     - media mini player (`mediasessd`)
     - capture hooks (stub OK)
   - deterministic, bounded error handling and no `unwrap/expect`
3. Host-first proof:
   - deterministic snapshot goldens for Launcher and Quick Settings (light/dark/HC)
   - profile fixtures are first-class rather than optional drift checks:
     - fixture inputs should come from the same declarative profile/shell manifest model used by runtime wiring,
       not from a second hardcoded test-only enum table
     - `profile=desktop` baseline
     - `profile=phone` / `profile=tablet` with portrait and landscape variants where the shell meaningfully adapts
     - `profile=tv` (10-foot spacing/typography), if SystemUI chooses to branch on `device.profile`
     - `profile=convertible` with at least `shellMode=desktop` and `shellMode=tablet` when shell posture changes layout/affordances
   - interaction tests:
     - search filters app list deterministically
     - toggles write prefs and roundtrip to initial state
     - volume slider calls audiod stub
     - media tile reflects mediasessd mock
   - launcher grid and QS tiles should use width-bucket-aware measurement so resize/profile changes do not force avoidable relayout cascades

## Non-Goals

- Kernel changes.
- Removing legacy SystemUI paths (v1 keeps legacy behind a feature flag).
- OS marker wiring/postflight (Phase 1b).

## Constraints / invariants (hard requirements)

- Parity-first: DSL UI must match legacy behavior and key visuals within documented tolerances.
- Deterministic snapshots and deterministic mocks.
- A11y labels/roles on all actionable elements.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/systemui_dsl_phase1_host/`:

- snapshot goldens for:
  - Launcher (light/dark/high-contrast)
  - Quick Settings (light/dark/high-contrast)
- add goldens for at least one non-desktop profile family and document the intended deltas
- interactions:
  - search reduces visible apps list deterministically
  - toggles update prefs and can be restored in teardown
  - volume slider invokes audiod adapter
  - media tile reflects mediasessd mock updates

## Touched paths (allowlist)

- `userspace/systemui/dsl/` (new)
- `userspace/systemui/dsl_bridge/` (new)
- `tests/systemui_dsl_phase1_host/` (new)
- `docs/systemui/dsl-migration.md` (Phase 1 notes; full doc in Phase 1b)

## Plan (small PRs)

1. DSL workspace scaffolding + page/component skeletons
2. dsl_bridge adapters + deterministic mock interfaces
3. host snapshots + interaction tests + goldens

## Handoff / sequencing note

- `TASK-0080B`/`TASK-0080C` establish the early visible shell and launcher for app testing in the `0081–0118` range
- `TASK-0119` then becomes the parity/polish/Quick Settings phase for the real Launcher + QS DSL surfaces
