---
title: TASK-0123 Notifications v2b (service): notifd channels/actions/visibility + persistence/history + unread/badging + host tests
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Notifications v2 minimal: tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - Persistence substrate: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy as Code (quotas/limits): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Logging/audit sink: tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
---

## Context

`TASK-0069` establishes **Notifications v2 minimal** (channels/actions/inline reply + basic SystemUI rendering) and explicitly defers persistence/history.

This task upgrades **notifd** into a production-grade service model while remaining kernel-unchanged and deterministic:

- channel registration semantics (per app),
- visibility (lock-screen policy hints),
- persistence of channel settings,
- history ring persistence,
- unread counts for launcher badging.

SystemUI rendering, DND scheduling, and lockscreen UI behavior are handled in follow-up tasks.

## Goal

Deliver:

1. `notifd` v2b data model:
   - channels per app (`Channel { id, name, importance, bypassDnd, showBadge }`)
   - notifications with:
     - `Priority`, `Visibility`
     - `actions[]` (including inline reply as a kind/stub)
     - grouping keys and timestamps
2. Persistence (via `/state`):
   - channels persisted under `state:/notifs/channels/<appId>.json`
   - history ring persisted as `state:/notifs/history.log` (JSONL)
3. Read APIs:
   - `list(limit)` for current notifications
   - `history(limit)` for history ring
   - `unreadCount(appId)` derived deterministically and bounded
4. Enforcement:
   - unregistered channel rejected with stable error (default channel `general` may be lazily created if documented)
   - bounded history and bounded per-notification size
   - deterministic IDs and stable ordering rules
5. Markers:
   - `notifd: ready`
   - `notif: post id=... app=... chan=... prio=...`
   - `notif: action id=... act=...`
   - `policy:notifs enforce on`

## Non-Goals

- Kernel changes.
- DND daemon and scheduling (separate task).
- Heads-up UI and inline reply UI (separate task).
- Lock-screen redaction UI (separate task; visibility is only a hint in this service layer).

## Constraints / invariants (hard requirements)

- Deterministic behavior (IDs, ordering, history rotation).
- Bounded RAM and bounded `/state` writes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/notifs_v2_host/`:

- channel registration is required (or `general` lazy behavior is consistent and documented)
- posting 3 notifications produces deterministic IDs and stable ordering
- history ring persists and rotates deterministically
- unread counts per app update correctly on cancel/dismiss
- action ack records payload deterministically (reply payload accepted but may be stub)

### Proof (OS/QEMU) — gated

UART markers include at least:

- `notifd: ready`
- `notif: post id=`
- `policy:notifs enforce on`

## Touched paths (allowlist)

- `source/services/notifd/`
- `docs/notifications/overview.md` (added/updated in later integration task if needed)
- `tests/notifs_v2_host/`

## Plan (small PRs)

1. notifd: model + IDL (or on-wire contract) + markers
2. persistence: channels + history ring (JSONL) with bounds
3. unread counts + host tests
