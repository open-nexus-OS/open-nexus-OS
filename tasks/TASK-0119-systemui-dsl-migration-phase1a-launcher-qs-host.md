---
title: TASK-0119 SystemUI→DSL Migration Phase 1a: Launcher + Quick Settings DSL pages + bridge + host snapshots/interactions
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v0.1 interpreter: tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
  - DSL v0.2 app mechanics (optional): tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
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

This phase must preserve behavior/visual parity and accessibility labels, and be proven via deterministic host tests.

OS wiring and postflight markers are handled in Phase 1b (`TASK-0120`).

## Goal

Deliver:

1. SystemUI DSL workspace:
   - `userspace/systemui/dsl/pages/Launcher.nx`
   - `userspace/systemui/dsl/pages/QuickSettings.nx`
   - reusable DSL components under `components/`
   - `themes/` mapping to existing tokens (light/dark/HC)
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
   - interaction tests:
     - search filters app list deterministically
     - toggles write prefs and roundtrip to initial state
     - volume slider calls audiod stub
     - media tile reflects mediasessd mock

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
