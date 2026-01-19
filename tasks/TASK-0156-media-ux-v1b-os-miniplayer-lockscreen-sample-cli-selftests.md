---
title: TASK-0156 Media UX v1b (OS/QEMU): SystemUI mini-player + lockscreen tile + notif integration (optional) + sample player + nx-media + selftests/postflight/docs
status: Draft
owner: @media
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Media apps product track (miniplayer/lockscreen shared across apps): tasks/TRACK-MEDIA-APPS.md
  - Media UX core: tasks/TASK-0155-media-ux-v1a-host-mediasessd-focus-nowplaying-artcache.md
  - Media baseline umbrella: tasks/TASK-0101-ui-v16c-media-sessions-systemui-controls.md
  - Lockscreen baseline: tasks/TASK-0109-ui-v18c-lockd-lockscreen-autolock.md
  - Notifications baseline: tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - Policy caps (apps/systemui): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With mediasessd core present (v1a), we can ship a deterministic offline “media UX” slice:

- a mini-player UI surface in SystemUI (tray),
- a lockscreen media tile,
- a tiny offline “sample player” that drives sessions (no real audio),
- a CLI for listing/controlling sessions,
- bounded OS selftests and postflight.

## Goal

Deliver:

1. SystemUI mini-player overlay (tray):
   - shows artwork/title/artist and transport controls
   - play/pause/next/prev/seek (seek step configurable; v1 uses fixed step)
   - keyboard shortcuts (when focused): Space play/pause, ←/→ seek ±5s, P/N prev/next
   - markers:
     - `miniplayer: open`
     - `miniplayer: control play|pause|next|prev|seek`
2. Lockscreen media tile:
   - shows now-playing session
   - transport controls wired to mediasessd
   - marker: `lockscreen: media tile open`
3. (Optional) notification integration:
   - media-style notification for the active session with transport actions
   - clicking focuses/opens mini-player
   - markers:
     - `notif: media show sid=<sid>`
     - `notif: media update`
     - `notif: media hide`
   - NOTE: only enable if notifd actions infrastructure exists; otherwise explicit `stub/placeholder`

Scope note (v2 follow-up):

- Session handoff UX, session switcher expansion, and notification action wiring are tracked as Media UX v2 (`TASK-0184`/`TASK-0185`).
4. Sample player app (deterministic, offline):
   - creates a session (`appId=com.example.media.sample`)
   - 3 seeded tracks from `pkg://media/sample/` metadata fixtures
   - simulated position ticking with injected clock (deterministic; bounded runtime)
   - markers:
     - `media-sample: play "<title>"`
     - `media-sample: pause`
5. CLI `nx media`:
   - list/active/play/pause/next/prev/seek/focus
   - stable output lines + markers like `nx: media play sid=<sid>`
6. Policy caps:
   - `media.session.publish` required to create/update sessions
   - `media.session.control` required for SystemUI/lockscreen/CLI control of other sessions
7. OS selftests (bounded, QEMU-safe):
   - `SELFTEST: media v1 playing ok`
   - `SELFTEST: media v1 pause ok`
   - `SELFTEST: media v1 seek ok`
   - `SELFTEST: media v1 focus ok`
8. Docs + postflight:
   - docs: overview + SystemUI behavior + integration + `nx-media`
   - postflight delegates to canonical proofs (host tests + `scripts/qemu-test.sh`)

## Non-Goals

- Kernel changes.
- Real audio output or decoding (handled by later media pipeline tasks).
- Full lockscreen security semantics (tile is UI control surface only).

## Constraints / invariants (hard requirements)

- Determinism: bounded timers, injected clock in sample, stable ordering.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: if notif integration is not wired, it must be explicit `stub/placeholder` (never “ok”).

## Red flags / decision points (track explicitly)

- **YELLOW (notif integration dependencies)**:
  - Media-style notifications depend on notifd action plumbing and SystemUI notification surface.
  - If missing, keep it out of v1 and document it as follow-up.

- **YELLOW (`/state` gating for artwork cache)**:
  - artwork persistence is gated; v1 can run RAM-only.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p media_ux_v1_host -- --nocapture`

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers (to be added to `scripts/qemu-test.sh` expected list):
    - `mediasessd: ready`
    - `SELFTEST: media v1 playing ok`
    - `SELFTEST: media v1 pause ok`
    - `SELFTEST: media v1 seek ok`
    - `SELFTEST: media v1 focus ok`

## Touched paths (allowlist)

- `source/services/mediasessd/`
- `userspace/systemui/overlays/mini_player/` (new)
- `userspace/systemui/lockscreen/tiles/media/` (new)
- `userspace/apps/media-sample/` (new)
- `tools/nx-media/` (new)
- `source/apps/selftest-client/`
- `tools/postflight-media-ux-v1.sh` (delegates)
- `docs/media/overview.md` + `docs/media/systemui.md` + `docs/media/integration.md`
- `docs/tools/nx-media.md`

## Plan (small PRs)

1. SystemUI mini-player + lockscreen tile wired to mediasessd
2. media-sample app + nx-media CLI
3. selftests + postflight + docs
4. notif integration only if dependencies exist (otherwise explicit follow-up)

## Acceptance criteria (behavioral)

- In QEMU, mini-player and lockscreen controls operate the sample session deterministically.
- Selftest prints all four OK lines and the postflight passes.
