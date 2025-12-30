---
title: TASK-0124 Notifications v2c (policy): dndd modes/schedules + notifd integration rules + tests/markers
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - notifd persistence/unread: tasks/TASK-0123-notifications-v2b-notifd-persistence-history-unread.md
  - Notifications v2 minimal: tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - Prefs store (user-configurable DND): tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Logging/audit sink: tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
---

## Context

Notifications v2 needs a Do-Not-Disturb policy source with deterministic schedules/modes.
This task introduces `dndd` as the policy authority and integrates it into `notifd` decisions.

UI rendering (heads-up banners, DND chips, settings UI) is a separate task.

## Goal

Deliver:

1. New service `dndd`:
   - modes:
     - `off`
     - `priorityOnly`
     - `alarmsOnly` (stub OK if alarms don’t exist yet; must be explicit)
     - `totalSilence`
   - schedules: weekday + start/end minutes (deterministic evaluation)
   - rule storage via prefs or `/state` (documented single source of truth)
2. Integration rules:
   - `notifd` queries `dndd.eval(ts)` to decide whether a notification is suppressed for heads-up
   - `bypassDnd` channels may bypass in all modes **except** `totalSilence`
   - stable, auditable deny reasons for suppression decisions
3. Markers:
   - `dndd: ready`
   - `dnd: mode <...>`
   - `dnd: active <true|false>`

## Non-Goals

- Kernel changes.
- Heads-up UI policy plumbing in SystemUI (separate task).
- Full “alarmsOnly” semantics if alarms are not implemented (v1 may treat it as `priorityOnly` but must be documented as a stub).

## Constraints / invariants (hard requirements)

- Deterministic time evaluation under test harness (injectable clock).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/notifs_v2_host/` (or `tests/dnd_host/`):

- schedules evaluate active inside window and inactive outside (including wrap-around end < start)
- `totalSilence` suppresses regardless of bypass flag
- `priorityOnly` allows bypass channels and blocks non-bypass deterministically
- audit events emitted for suppressed heads-up decisions (via log sink)

### Proof (OS/QEMU) — gated

UART markers include:

- `dndd: ready`
- `dnd: mode `

## Touched paths (allowlist)

- `source/services/dndd/` (new)
- `source/services/notifd/` (integration)
- `tests/notifs_v2_host/` (extend)

## Plan (small PRs)

1. dndd: data model + eval + markers
2. notifd: policy integration + stable reasons
3. host tests

