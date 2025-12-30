---
title: TASK-0065 UI v6b: appmgrd ability-lite lifecycle + SystemUI navigation + notifd toasts/notifications
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v6a WM baseline: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - Process-per-service: docs/rfcs/RFC-0002-process-per-service-architecture.md
  - Execd supervisor: source/services/execd/
  - Config broker (notification limits): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (launch/notify guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Updates (future integration): tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With a WM in place (v6a), we can implement a minimal “Ability-Lite” lifecycle broker (`appmgrd`)
and wire SystemUI navigation and notifications.

This is intentionally userspace-only: app lifecycle is brokered, not kernel-enforced.

## Goal

Deliver:

1. `appmgrd` lifecycle broker:
   - launch apps via `execd`
   - lifecycle callbacks: Create → Start → Foreground/Background → Suspend/Resume → Stop
   - recents list with metadata (thumbnails can be stubbed initially)
   - mediation with `windowd` WM: open window, bind surface, focus transitions
2. Notifications/toasts:
   - minimal `notifd` service (or extend if already present)
   - rate limit per app and priority
   - SystemUI plugin shows toasts and a small tray/shade stub
3. SystemUI navigation:
   - Back/Home/Recents stubs
   - focus switching across windows via WM/appmgrd
4. Host tests and OS markers.

## Non-Goals

- Kernel changes.
- Full multi-window apps.
- Real thumbnail capture pipeline (can be a follow-up once screencopy exists).

## Constraints / invariants (hard requirements)

- Deterministic lifecycle ordering and bounded timeouts.
- Policy guardrails:
  - only `appmgrd` may spawn apps
  - notification quotas per app
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (lifecycle authority)**:
  - This is userspace policy/contract; if an app ignores callbacks it can misbehave.
  - We must document this as “cooperative lifecycle” until stronger confinement exists.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v6b_host/`:

- lifecycle: mocked app receives callbacks in correct order (Create→Start→FG, BG/FG roundtrip)
- notifications: rate limiting drops are counted deterministically
- navigation: recents list and focus selection logic behaves as expected

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `appmgrd: ready`
- `appmgrd: launch (app=..., pid=...)`
- `appmgrd: fg (win=...)` / `bg (win=...)`
- `notifd: ready`
- `systemui: nav ready`
- `systemui: toast (app=..., id=...)`
- `notes: started` / `notes: paused` / `notes: resumed` (demo app)
- `SELFTEST: ui v6 launch ok`
- `SELFTEST: ui v6 lifecycle ok`
- `SELFTEST: ui v6 toast ok`

## Touched paths (allowlist)

- `source/services/appmgrd/` (new)
- `source/services/notifd/` (new or extend)
- `source/services/windowd/` (WM integration hooks)
- `source/services/execd/` (spawn wiring)
- `source/services/samgrd/` (service discovery as needed)
- `source/apps/selftest-client/` (markers)
- `userspace/apps/notes/` (demo app, minimal)
- `tests/ui_v6b_host/` (new)
- `tools/postflight-ui-v6b.sh` (delegates)
- `docs/ui/lifecycle.md` + `docs/ui/notifications.md` (new)

## Plan (small PRs)

1. appmgrd skeleton + lifecycle callbacks + markers
2. WM integration (open/bind/focus) + recents list (thumbnails stub)
3. notifd + SystemUI toast host + rate limiting + markers
4. demo app `notes` + launcher WM binding (minimal)
5. tests + OS selftest + docs + postflight

## Follow-ups

- Ability/Lifecycle v1.1 (backoff/crash-loop/kill reasons/FG-BG policies): `TASK-0234` (host-first) and `TASK-0235` (OS extension of appmgrd).

