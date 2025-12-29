---
title: TASK-0185 Media UX v2b (OS/QEMU): tray mini-player (session switcher) + notif actions wiring + deterministic clock UI + sample publishers + selftests/docs
status: Draft
owner: @media
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Media UX v1 OS slice: tasks/TASK-0156-media-ux-v1b-os-miniplayer-lockscreen-sample-cli-selftests.md
  - Media UX v2 core semantics: tasks/TASK-0184-media-ux-v2a-host-handoff-playerctl-deterministic-clock.md
  - Notifications actions baseline (service+UI): tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - notifd persistence/actions (service): tasks/TASK-0123-notifications-v2b-notifd-persistence-history-unread.md
  - Notifications SystemUI integration (actions UI): tasks/TASK-0125-notifications-v2d-systemui-headsup-redaction-badging-settings.md
  - Perf sessions (optional): tasks/TASK-0172-perf-v2a-perfd-sessions-stats-export.md
  - Policy caps: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Media UX v1 already ships a mini-player and sample player (offline). Media UX v2 upgrades:

- multi-session display and session switcher UI,
- session handoff UX (“Take over” when another app is playing),
- now-playing notifications with action wiring,
- and a deterministic playback clock surface (no audio stack).

## Goal

Deliver:

1. SystemUI tray mini-player v2:
   - shows artwork/title/artist, timeline, and transport controls
   - expandable session switcher list (from `mediasessd.list()`)
   - handoff UX:
     - when mediasessd reports a handoff offer, show “Take over” action
   - keyboard shortcuts:
     - Space toggle, arrows seek ±step, up/down prev/next, `S` stop
   - markers:
     - `tray: mini-player open`
     - `tray: mini-player control=<act>`
     - `tray: session switch sid=<sid>`
     - `tray: handoff take-over sid=<sid>`
2. Notifications integration (gated):
   - when active session meta changes, post/update a now-playing notification with actions
   - actions route to mediasessd control path
   - markers:
     - `notif: now-playing "<title>"`
     - `notif: action=<act>`
   - **must be explicitly gated**:
     - only enable once notifd actions + SystemUI action handling exists (`TASK-0069`/`TASK-0125`)
     - otherwise emit explicit `stub/placeholder` markers (never “ok”)
3. Sample publishers (deterministic):
   - `media-sample` and `podcast-sample` publish sessions and handle controls
   - deterministic tick behavior using injected monotonic clock (bounded)
4. CLI `nx media`:
   - list/now/activate/play/pause/toggle/seek
   - stable output; no QEMU dependency (selftests must not require running host CLIs inside QEMU)
5. Policy caps:
   - `media.session.publish` for apps
   - `media.session.control` for SystemUI/notifications/CLI control of other sessions
6. OS selftests (bounded):
   - `SELFTEST: media v2 play/pause ok`
   - `SELFTEST: media v2 seek ok`
   - `SELFTEST: media v2 handoff ok`
   - `SELFTEST: media v2 notif control ok` (only when notif actions are truly wired; otherwise explicit placeholder)
7. Docs:
   - v2 semantics, handoff UX, notification gating, and deterministic clock model

## Non-Goals

- Kernel changes.
- Real audio output, mixing, or decoding.

## Constraints / invariants (hard requirements)

- Determinism: injected clock; stable ordering; bounded UI updates.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: notification selftest must not claim “ok” unless notif action roundtrip actually worked.

## Red flags / decision points (track explicitly)

- **RED (notif actions gating)**:
  - Notification action controls depend on `TASK-0069` + `TASK-0125`. Until then, the v2 notif portion must be explicitly stubbed.

- **YELLOW (perfd hooks gating)**:
  - perfd session hooks are optional and must be gated on `TASK-0172` (or earlier perfd tasks).

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p media_ux_v2_host -- --nocapture` (from v2a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers:
    - `mediasessd: ready v2`
    - `SELFTEST: media v2 play/pause ok`
    - `SELFTEST: media v2 seek ok`
    - `SELFTEST: media v2 handoff ok`
    - `SELFTEST: media v2 notif control ok` (only if notif actions are wired; otherwise explicit placeholder)

## Touched paths (allowlist)

- `userspace/systemui/tray/mini_player/` (new or refactor existing mini-player)
- `source/services/mediasessd/` (OS wiring + markers)
- `userspace/apps/media-sample/` + `userspace/apps/podcast-sample/`
- `tools/nx-media/`
- `source/apps/selftest-client/`
- `docs/media/` + `docs/tools/nx-media.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. mini-player v2 UI (session switcher + handoff UX) + markers
2. sample publishers + handoff scenario + selftests
3. notif integration only when dependencies exist; otherwise explicit stub
4. docs + marker contract update

## Acceptance criteria (behavioral)

- In QEMU, session switching and handoff behavior are proven deterministically; notification controls are proven only when notif actions are available.
