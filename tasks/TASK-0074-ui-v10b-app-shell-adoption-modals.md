---
title: TASK-0074 UI v10b (OS-gated): App Shell + modal manager + toast unification + SystemUI/apps adoption + markers
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Design kit baseline: tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
  - WM baseline: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - UI runtime/animation baseline: tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
  - Notifications baseline: tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - Search/settings baseline: tasks/TASK-0071-ui-v9a-searchd-command-palette.md
  - Prefs/settings baseline: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Once primitives exist (v10a), we need consistent application chrome and systematic adoption:

- a reusable App Shell (title bar/toolbar/content/navigation slots),
- a modal manager contract for Dialog/Sheet with focus traps,
- unified toast rendering,
- and migration of SystemUI and key apps (launcher/notes/settings) to the kit.

This task is OS-gated because it touches running services and QEMU markers.

## Goal

Deliver:

1. `userspace/ui/app_shell`:
   - standard chrome and layout slots
   - hooks into WM title/icon state
   - integrates global shortcuts where appropriate (delegate to SystemUI)
2. Modal manager:
   - userspace-only modal stack (Dialog/Sheet) with backdrop, focus trap, ESC handling
   - consistent toasts via kit `ToastView`
3. Adoption/migration:
   - SystemUI overlays (quick settings, notifications, palette, settings overlay) use kit primitives
   - apps `launcher`, `notes`, `settings` adopt App Shell + kit controls
4. Markers + OS selftests + postflight.

## Non-Goals

- Kernel changes.
- Perfect “final UI”. This is v1 design system adoption with stable visuals and behavior.

## Constraints / invariants (hard requirements)

- Migration must not break existing markers; any new markers are additive and deterministic.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Modal manager must be bounded (cap stack depth).

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Goldens for App Shell chrome in light/dark (can be in `ui_v10_goldens` crate).

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `design: kit adopted (systemui)`
- `design: kit adopted (launcher)`
- `design: kit adopted (notes)`
- `SELFTEST: ui v10 button ok`
- `SELFTEST: ui v10 dialog ok`
- `SELFTEST: ui v10 theme recolor ok`

## Touched paths (allowlist)

- `userspace/ui/app_shell/` (new)
- SystemUI plugins (adoption)
- `userspace/apps/launcher/`, `userspace/apps/notes/`, `userspace/apps/settings/` (adoption)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v10.sh` (delegates)
- `docs/ui/app-shell.md` + `docs/ui/testing.md` (extend)

## Plan (small PRs)

1. app_shell crate + host snapshots
2. modal manager + unified toasts
3. SystemUI migration + markers
4. app migrations (launcher/notes/settings) + markers
5. OS selftests + docs + postflight

