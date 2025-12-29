---
title: TASK-0184 Media UX v2a (host-first): mediasessd multi-session + deterministic playback clock + handoff policy + playerctl client + tests
status: Draft
owner: @media
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Media UX v1 core: tasks/TASK-0155-media-ux-v1a-host-mediasessd-focus-nowplaying-artcache.md
  - Notifications actions baseline (for later wiring): tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - Perf sessions (optional): tasks/TASK-0172-perf-v2a-perfd-sessions-stats-export.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Media UX v1 (`TASK-0155/0156`) delivers a deterministic now-playing substrate plus a simple SystemUI control surface.
This v2 slice tightens the service semantics and adds:

- multi-session behavior (with stable ordering),
- a deterministic playback clock model,
- explicit handoff semantics between apps,
- and a typed client façade (`playerctl`) for UI/tools.

OS UI wiring, notifications, and sample apps are handled in `TASK-0185`.

## Goal

Deliver:

1. `mediasessd` v2 semantics (host-first):
   - multiple sessions registered concurrently
   - exactly one active session at a time (explicit `setActive`)
   - deterministic playback clock:
     - session reports `Position { ms, playing, tsNs }` (monotonic timestamp)
     - `nowPlaying()` returns computed live position as:
       - \( ms + (nowNs - tsNs) / 1e6 \) when `playing=true`, clamped to duration
     - injected monotonic clock for tests (no wallclock dependence)
2. Handoff policy:
   - if a new session registers while another is active and **playing**:
     - do not preempt; emit a deterministic “handoff offer” state
   - if current is **paused**:
     - auto-activate the new session
   - explicit “take over”/accept API surface (exact shape to be chosen; must be deterministic)
3. `userspace/libs/playerctl`:
   - typed helpers: play/pause/toggle/seek/next/prev/activate/query_now_playing
   - bounds checks (seek clamps) and stable error mapping
4. Markers (rate-limited):
   - `mediasessd: ready v2`
   - `media: register app=<id> sid=<sid>`
   - `media: active sid=<sid>`
   - `media: handoff offer from=<old> to=<new>`
   - `media: handoff accept sid=<sid>`
5. Deterministic host tests (`tests/media_ux_v2_host/`):
   - register + deterministic clock math (±1 ms window under injected clock)
   - pause stops clock progression
   - seek clamps deterministically
   - handoff rules (paused auto, playing requires explicit accept)
   - list ordering stable (e.g., active first, then appId/sid tie-break)

## Non-Goals

- Kernel changes.
- Real audio output/decoding.
- Notification delivery or UI surfaces (v2b).

## Constraints / invariants (hard requirements)

- Deterministic behavior: injected monotonic clock; stable ordering and tie-breakers.
- Bounded data: caps on sessions, metadata lengths, and callback/event queue sizes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **YELLOW (callback vs polling model)**:
  - App callback interfaces can introduce reentrancy hazards.
  - Prefer a deterministic command routing model and a “changed” signal suitable for polling unless cap-transfer/callback plumbing is proven safe.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p media_ux_v2_host -- --nocapture`

## Touched paths (allowlist)

- `source/services/mediasessd/` (extend)
- `userspace/libs/playerctl/` (new)
- `tests/media_ux_v2_host/` (new)

## Plan (small PRs)

1. mediasessd deterministic clock + tests
2. handoff policy + tests
3. playerctl library + tests

## Acceptance criteria (behavioral)

- Host tests prove deterministic clock/control/handoff semantics and stable ordering.

Follow-up:

- Media UX v2.1 adds a deterministic audio engine stub (`audiod`) plus focus/ducking and per-app volume/mute integration with SystemUI. Tracked as `TASK-0217`/`TASK-0218`.
