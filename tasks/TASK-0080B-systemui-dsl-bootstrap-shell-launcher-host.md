---
title: TASK-0080B SystemUI DSL bootstrap shell (host-first): desktop background + launcher + app launch contract
status: Draft
owner: @ui
created: 2026-03-28
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL visible mount baseline: tasks/TASK-0076B-dsl-v0_1c-visible-os-mount-first-frame.md
  - DSL v0.3 demo/perf task: tasks/TASK-0080-dsl-v0_3b-perf-bench-os-aot-demo.md
  - DSL profile/runtime contract: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - App lifecycle baseline: tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - SystemUI DSL migration phase 1a: tasks/TASK-0119-systemui-dsl-migration-phase1a-launcher-qs-host.md
---

## Context

By the time DSL v0.3 exists, we should not still be testing apps only through demo tiles and markers.
We need a **real but minimal SystemUI shell** that can host app launch from a visible desktop before the broader
SystemUI migration is complete.

This task intentionally extracts the **Launcher-first** slice from the later SystemUI DSL migration so app tasks in
the `0081–0118` range can be tested against a visible shell.

## Goal

Deliver:

1. Bootstrap SystemUI DSL workspace:
   - `userspace/systemui/dsl/pages/BootstrapShellPage.nx`
   - `userspace/systemui/dsl/pages/LauncherPage.nx`
   - `userspace/systemui/dsl/components/**.nx`
   - `userspace/systemui/dsl/composables/**.nx`
   - optional `userspace/systemui/dsl/services/**.nx` effect adapters
2. Visible bootstrap shell:
   - deterministic background/wallpaper
   - launcher grid/list of apps from `appmgrd`
   - app launch action wiring
   - host fixtures may force a small baseline set of profiles/orientations (desktop, phone/tablet portrait/landscape)
     so the bootstrap shell is profile-aware from the start instead of becoming desktop-only by accident
3. Canonical DSL page structure:
   - page files follow the `Store` + `Event` + `reduce` + `@effect` + `Page` shape from `TASK-0075`
   - pure state logic stays pure; service calls only in effects
4. Host-first proof:
   - snapshots for bootstrap shell and launcher
   - interaction tests for search/filter/app launch request emission

## Non-Goals

- Full Quick Settings migration.
- Notifications Center.
- Media mini-player.
- Session/login/auth.

## Constraints / invariants (hard requirements)

- This is a real SystemUI shell path, not a temporary side app.
- Launcher page becomes the base for `TASK-0119` rather than a disposable prototype.
- App launch uses the real app lifecycle/service contract, not mock-only shell behavior.
- Deterministic host fixtures for app list and shell state.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- bootstrap shell/launcher DSL snapshots are stable
- search/filter is deterministic
- launcher tap emits the expected launch request

## Touched paths (allowlist)

- `userspace/systemui/dsl/`
- `userspace/systemui/dsl_bridge/`
- `tests/systemui_bootstrap_shell_host/` (new)
- `docs/systemui/dsl-migration.md`
- `docs/dev/dsl/overview.md`

## Plan (small PRs)

1. bootstrap shell page + launcher page
2. bridge adapters for app list/launch
3. host snapshots + interactions
4. handoff to `TASK-0080C` and `TASK-0119`
