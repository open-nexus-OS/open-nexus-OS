---
title: TASK-0101 UI v16c: mediasessd (now-playing + controls) + SystemUI volume OSD + tray/lock controls + media keys
status: Draft
owner: @media
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Audiod mixer: tasks/TASK-0100-ui-v16b-audiod-mixer.md
  - Notifications actions baseline: tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - Policy as Code (session focus): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
---

## Context

Media UX needs a system-level “now playing” and control surface:

- a single active media session at a time (v1),
- metadata and playback state updates,
- SystemUI controls (tray/lock) and volume OSD,
- media key routing to the active session only.

Apps integrate in v16d.

Scope note:

- A small, QEMU-tolerant “Media UX v1” slice (mediasessd core + mini-player + lockscreen tile + sample app + CLI + selftests)
  is tracked as `TASK-0155` (host-first mediasessd core) and `TASK-0156` (OS wiring + proofs).
  This task remains the v16c umbrella; implementation should not duplicate work already done by those tasks.

## Goal

Deliver:

1. `mediasessd` service:
   - register session per app
   - update metadata and playback state/position
   - accept control commands (play/pause/next/prev/seek/stop)
   - single active session focus rules (v1)
   - markers:
     - `mediasessd: ready`
     - `media: active sid=...`
2. SystemUI integration:
   - volume OSD driven by `audiod.mixLevel` (and mute/vol knobs if exposed)
   - tray widget and lock-screen controls bound to `mediasessd.control`
   - media keys (Play/Pause/Next/Prev/Vol+/Vol-/Mute) routed deterministically
   - markers:
     - `systemui: volume osd`
     - `systemui: media widget on`
3. Host tests for mediasessd focus and control routing (mocked clients).

## Non-Goals

- Kernel changes.
- Multiple simultaneous active sessions (v2+).
- Real lock-screen security model (stub UI controls only).

## Constraints / invariants

- Deterministic session focus selection.
- Bounded metadata sizes and queue lengths.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v16c_host/`:

- register two sessions → focus switches deterministically
- control commands delivered to active session only
- state updates reflected in subscriber stream deterministically

## Touched paths (allowlist)

- `source/services/mediasessd/` (new)
- SystemUI plugins (tray/lock widget + OSD)
- `tests/ui_v16c_host/`
- `docs/media/sessions.md` (new)

## Plan (small PRs)

1. mediasessd core + markers + host tests
2. SystemUI tray/lock controls + volume OSD + markers
3. docs
