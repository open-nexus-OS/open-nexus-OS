---
title: TASK-0125 Notifications v2d (SystemUI): heads-up + actions/reply UI + lock-screen redaction wiring + history/badging + tests/markers/docs
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Notifications v2 minimal: tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - notifd persistence/unread: tasks/TASK-0123-notifications-v2b-notifd-persistence-history-unread.md
  - dndd policy: tasks/TASK-0124-notifications-v2c-dndd-modes-schedules-policy.md
  - Lock screen + redaction baseline: tasks/TASK-0109-ui-v18c-lockd-lockscreen-autolock.md
  - SystemUI→DSL migration (Notif Center in DSL): tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - SystemUI→DSL OS wiring: tasks/TASK-0122-systemui-dsl-migration-phase2b-os-wiring-postflight-docs.md
---

## Context

This task upgrades the user experience and OS proof for notifications:

- heads-up banners,
- actions + inline reply (stub OK, must be explicit),
- lock-screen redaction wiring based on `Visibility`,
- history presentation,
- app icon badging in Launcher,
- Settings pages for Notifications + DND.

Service-side semantics are delivered by `TASK-0123` and `TASK-0124`.

## Goal

Deliver:

1. Heads-up banners:
   - show for `Priority.high|urgent` unless suppressed by DND
   - auto-dismiss timeout (configurable)
   - swipe-to-dismiss and action buttons
   - marker: `notifs: headsup show id=...`
2. Notification Center (DSL) v2:
   - enable dismiss and clear-all
   - grouping (app/channel) and history view
   - inline reply UI row that calls `ackAction(...,"reply",payload)` (payload delivery may be stub)
   - markers:
     - `notifs: dismissed id=...`
     - `notifs: inline-reply id=...`
3. Lock-screen redaction wiring:
   - when locked:
     - `Visibility.private`: title visible, body redacted
     - `Visibility.secret`: title and body redacted
     - `Visibility.public`: full
   - marker: `notifs: redacted (id=...)`
   - **must not duplicate** `TASK-0109` ownership; this task only wires notif visibility into the lockscreen UI path.
4. Badging:
   - Launcher (DSL) shows unread badge based on `notifd.unreadCount(appId)`
   - enforce `showBadge=true` on channel
   - marker: `launcher: badge app=... n=...`
5. Settings pages (DSL):
   - Notifications: per-app channel settings (importance, badge allowed, bypass DND)
   - DND: mode selector + schedule editor + allow repeat callers
   - markers:
     - `settings:notifs apply (...)`
     - `settings:dnd apply (...)`
6. Docs + tests + OS markers:
   - deterministic host tests for heads-up suppression rules and redaction behavior
   - OS selftests for:
     - heads-up show + dismiss
     - DND suppression vs bypass
     - lock-screen redaction markers
     - launcher badging updates

## Non-Goals

- Kernel changes.
- Full “reply delivery” into arbitrary apps (may be stubbed; must be explicit).
- Rich notification content (images/progress) and remote sync.

## Constraints / invariants (hard requirements)

- Deterministic tests and deterministic markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/notifs_v2_host/` (extended) or `tests/notifs_systemui_host/`:

- heads-up shown for high/urgent and suppressed under DND except bypass rule (and never in totalSilence)
- lock redaction mapping matches visibility
- badging counts derived correctly from unreadCount and showBadge

### Proof (OS/QEMU) — gated

UART includes:

- `SELFTEST: notifs v2 headsup ok`
- `SELFTEST: notifs v2 dnd ok`
- `SELFTEST: notifs v2 lock redaction ok`
- `SELFTEST: notifs v2 badging ok`

## Touched paths (allowlist)

- SystemUI notification surfaces (heads-up + center + launcher)
- `source/apps/selftest-client/`
- `tools/postflight-notifs-v2.sh`
- `docs/notifications/overview.md`
- `docs/notifications/dnd.md`
- `docs/systemui/notifications.md`

## Plan (small PRs)

1. heads-up overlay + markers
2. DSL notification center v2 actions + inline reply UI (stub payload OK) + markers
3. lock-screen redaction wiring + markers
4. launcher badging + settings pages for notifs/dnd
5. tests + selftests + postflight + docs
