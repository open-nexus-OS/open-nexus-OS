---
title: TASK-0069 UI v8a: Notifications v2 (channels + actions + inline reply) + SystemUI rendering + tests/markers
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - System Delegation / System Surfaces (messaging reply routing): tasks/TRACK-SYSTEM-DELEGATION.md
  - UI v6b notifications baseline: tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - UI v3b IME/text-input baseline (inline reply widget): tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - Policy as Code (notif quotas): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (notif settings): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Logging/audit sink: tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

UI v6 introduced minimal notifications/toasts. UI v8a upgrades notifications into a more useful system:

- channels with importance and rate limits,
- actionable notifications (buttons),
- inline reply for messaging-style notifications,
- SystemUI rendering for actions and reply field.

System Delegation note:
- Inline reply should remain a system surface; for messaging apps this can later dispatch to action-based intents
  (e.g. `chat.compose`/`chat.reply`) instead of each app inventing separate reply routing semantics.

WM resize/move and shortcuts/settings overlays are in v8b (`TASK-0070`).

## Goal

Deliver:

1. `notifd` v2 API:
   - `notify(appId,title,body,channel,actions,inlineReply)`
   - `action(id,actionId)`
   - `reply(id,text)`
   - `cancel(id)`
   - `channels()`
2. Channel model:
   - `{system, default, messaging}` initially
   - per-channel importance and per `appId+channel` token-bucket rate limiting
3. SystemUI rendering:
   - banner/toast supports action buttons
   - inline reply text field (Enter to submit)
   - notification center shade stub groups by channel
4. Deterministic host tests and OS selftest markers.

## Non-Goals

- Kernel changes.
- Full notification persistence/history across reboots (follow-up `TASK-0123`).
- Full rich content (images, progress bars).
- Production-grade DND schedules/modes, lock-screen redaction semantics, heads-up policy, and launcher badging (follow-ups `TASK-0124` + `TASK-0125`).

## Constraints / invariants (hard requirements)

- Deterministic rate limiting and stable deny reasons.
- Bounded memory:
  - cap number of in-memory notifications
  - cap inline reply length
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v8a_host/`:

- posting notification with actions + inline reply yields deterministic ID and stored state
- triggering action emits expected audit/log event
- submitting reply emits expected audit/log event (and length is enforced)
- rate limit per app+channel works deterministically

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `notifd: ready`
- `notifd: post (id=.. ch=.. actions=N reply=bool)`
- `notifd: action (id=.. action=..)`
- `notifd: reply (id=.. len=..)`
- `systemui: notif action rendered`
- `systemui: notif reply ready`
- `SELFTEST: ui v8 action ok`
- `SELFTEST: ui v8 reply ok`

## Touched paths (allowlist)

- `source/services/notifd/` (extend v2)
- `source/services/windowd/` + SystemUI plugins (render actions/reply)
- `tests/ui_v8a_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v8a.sh` (delegates)
- `docs/dev/ui/notifications-v2.md`

## Plan (small PRs)

1. notifd: channel model + IDL + rate limit + markers
2. SystemUI: render actions + inline reply field + markers
3. host tests + OS selftest markers + docs + postflight

## Follow-ups (v2 “deluxe” / production semantics)

- `TASK-0123`: notifd persistence/history/unread counts (badging substrate)
- `TASK-0124`: dndd modes/schedules + notifd integration rules
- `TASK-0125`: SystemUI heads-up + lock-screen redaction wiring + badging + settings pages + OS proofs
