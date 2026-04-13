---
title: TASK-0121 SystemUI→DSL Migration Phase 2a: Settings core pages + Notifications Center (read-only) + bridge extensions + host tests
status: Draft
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Phase 1a (Launcher+QS DSL pages): tasks/TASK-0119-systemui-dsl-migration-phase1a-launcher-qs-host.md
  - Phase 1b (OS wiring): tasks/TASK-0120-systemui-dsl-migration-phase1b-os-wiring-postflight.md
  - DSL interpreter baseline: tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
  - UI layout pipeline contract: docs/dev/ui/foundations/layout/layout-pipeline.md
  - Settings substrate (typed prefs): tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - Notifications v2 baseline: tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - A11y suite baseline: tasks/TASK-0114-ui-v20a-a11yd-tree-actions-focusnav.md
---

## Context

We are continuing the SystemUI → DSL migration. Phase 2 targets:

- Settings (core pages)
- Notifications Center (read-only)
- an accessibility audit (labels/roles + focus order)

This task is **host-first**: it delivers the DSL pages and deterministic host tests.
OS wiring and postflight markers are handled in Phase 2b (`TASK-0122`).

Settings-tree note:

- `TASK-0121` establishes the canonical SystemUI DSL Settings tree under `userspace/systemui/dsl/pages/settings/`
- later specialized Settings tasks (Accessibility, Security & Privacy, Updates, Language & Region, Notifications/DND extensions)
  must extend this same tree and the same `userspace/systemui/dsl_bridge/` adapters rather than creating separate settings roots

## Goal

Deliver:

1. Extend `userspace/systemui/dsl_bridge` with:
   - settings get/set over `settingsd` (typed: bool/int/float/string/enum/json) and system info summary
   - volume/dnd helpers
   - notifications read-only list + subscribe stream
   - deterministic mocks under `cfg(test)`
   - markers:
     - `bridge: settings ready`
     - `bridge: notifications ready`
2. Settings DSL pages under `userspace/systemui/dsl/pages/settings/`:
   - `Settings.nx` index + sidebar navigation
   - `Display.nx`, `Sound.nx`, `Privacy.nx`, `Accessibility.nx`, `System.nx`
   - this is the canonical Settings page tree that later tasks extend in-place
   - known follow-on specializations should plug into this tree rather than creating parallel settings roots:
     - Accessibility (`TASK-0118`)
     - Security & Privacy (`TASK-0137`)
     - Updates (`TASK-0140`)
     - Language & Region (`TASK-0175`)
   - related page-local `Store`/`Event`/`reduce`/`@effect` blocks may be colocated in each page file while the feature is small
   - shared pure logic belongs under `userspace/systemui/dsl/composables/`; service adapters belong under `userspace/systemui/dsl/services/`
   - bind controls to bridge calls; show immediate state echo
   - a11y: role/name for all actionable controls; stable focus order
   - markers:
     - `systemui:dsl settings on`
     - `settings:dsl page <name> on`
3. Notifications Center DSL page (read-only):
   - `userspace/systemui/dsl/pages/notifications/NotifCenter.nx`
   - canonical DSL structure from `TASK-0075` applies here too (`Store`/`Event`/`reduce`/`@effect`/`Page`)
   - virtualized list rows; filters by app/channel
   - actions disabled (v1 read-only enforced)
   - live updates via subscribe
   - stable anchor-by-key and bounded mixed-height measurement caches for notification rows
   - a11y list/listItem roles and polite announce for new notifications (when focused inside)
   - markers:
     - `systemui:dsl notifs on`
     - `notifs:dsl listed n=<count>`
   - this page is the canonical Notification Center DSL root that later notification tasks extend (actions, history, DND, badging)
   - known follow-on extensions:
     - Notifications v2d heads-up/history/badging/settings (`TASK-0125`)
4. Deterministic host tests:
   - snapshots (light/dark/high-contrast) for settings pages
   - prefs roundtrip asserts bridge writes and UI reads
   - notif list/filtering deterministic
   - settings deep-link/focus targets and notification scroll anchors remain stable across resize/filter changes
   - a11y audit: every actionable node has non-empty role/name; tab order matches expected vector

## Non-Goals

- Kernel changes.
- Implementing notif actions/dismiss/snooze (read-only v1).
- OS marker wiring/postflight (Phase 2b).

## Constraints / invariants (hard requirements)

- Parity-first UX: match legacy behavior within documented limits.
- Deterministic goldens and deterministic mocks.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/systemui_dsl_phase2_host/`:

- PNG snapshots for settings pages (light/dark/HC) match goldens (SSIM threshold documented)
- prefs roundtrip toggles and slider writes are observed and UI reflects them
- notif list/filter renders deterministic IR JSON and stable visible count
- a11y audit passes for settings page (role+name present; focus order expected)

## Touched paths (allowlist)

- `userspace/systemui/dsl_bridge/` (extend)
- `userspace/systemui/dsl/pages/settings/` (new)
- `userspace/systemui/dsl/pages/notifications/` (new)
- `tests/systemui_dsl_phase2_host/` (new)

## Plan (small PRs)

1. bridge extensions + deterministic mocks + markers
2. settings pages (DSL) + a11y labels + markers
3. notifications center (DSL) + read-only enforcement + markers
4. host snapshots + prefs/notifs tests + a11y audit
