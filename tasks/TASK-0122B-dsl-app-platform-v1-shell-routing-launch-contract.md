---
title: TASK-0122B DSL App Platform v1: shared app shell + launch/open contract + host proofs
status: Draft
owner: @ui
created: 2026-03-28
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - SystemUI DSL Phase 2b: tasks/TASK-0122-systemui-dsl-migration-phase2b-os-wiring-postflight-docs.md
  - App shell baseline: tasks/TASK-0074-ui-v10b-app-shell-adoption-modals.md
  - DSL v0.2 app mechanics: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - Document picker/openWith app contract: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
---

## Context

After SystemUI itself is visibly mounted through DSL, the next source of drift is app-by-app shell code.
Files, Text, Images, Notes, Accounts, and similar apps should not each invent their own launch/open/titlebar/focus
pattern.

We need one shared DSL app platform contract before the app wave lands.

## Goal

Deliver:

1. Shared DSL app shell contract:
   - common titlebar / toolbar / content slots
   - standard window title and icon wiring
   - common empty/loading/error surfaces
2. Shared launch/open contract:
   - launch args
   - optional `openUri`
   - restore/focus semantics for single-window vs multi-window capable apps
3. Canonical DSL app structure:
   - app pages and components follow the `TASK-0075` layout
   - page-level files may colocate `Store`, `Event`, `reduce`, `@effect`, and `Page`
   - larger apps may extract pure stores to `ui/composables/**.store.nx` and service adapters to `ui/services/**.service.nx`
   - app shell logic must consume resolved profile/shell IDs from the declarative manifest/runtime contract rather than
     assuming a closed built-in set of profiles
4. Host-first proof:
   - app shell snapshots
   - launch/open fixture tests

## Non-Goals

- Implementing all apps.
- Full desktop/window manager policy.
- Deep file/content/media adapters (separate integration kit task).

## Constraints / invariants (hard requirements)

- No per-app reinvention of app chrome.
- No direct service IO from reducers or view code.
- Launch/open semantics must be deterministic and documented.
- App shell routing must not hardcode the upstream starter profile list as the only valid universe.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- app shell snapshots stable across themes
- fixture-driven launch/open flows deterministic

## Touched paths (allowlist)

- shared DSL app shell package(s)
- app platform docs
- `tests/dsl_app_platform_host/` (new)
- `docs/dev/dsl/overview.md`
- `docs/dev/ui/app-shell.md`

## Plan (small PRs)

1. shell contract and docs
2. launch/open contract fixtures
3. snapshots/tests + handoff to app tasks
