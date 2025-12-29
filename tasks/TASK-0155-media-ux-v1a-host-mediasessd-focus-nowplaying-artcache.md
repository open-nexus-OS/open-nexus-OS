---
title: TASK-0155 Media UX v1a (host-first): mediasessd sessions + focus rules + now-playing + artwork cache (bounded) + deterministic tests
status: Draft
owner: @media
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Media baseline task (umbrella): tasks/TASK-0101-ui-v16c-media-sessions-systemui-controls.md
  - Audiod mixer (volume OSD later): tasks/TASK-0100-ui-v16b-audiod-mixer.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Quotas v1 (cache bounds later): tasks/TASK-0133-statefs-quotas-v1-accounting-enforcement.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic, offline “now playing” substrate that SystemUI and apps can rely on:

- sessions with metadata and playback state,
- local focus rules (v1: single “gain” owner),
- a now-playing selection policy,
- bounded artwork caching (RAM first; `/state` later).

This is the host-first, engine/core slice. OS UI wiring is in `TASK-0156`.

## Goal

Deliver:

1. `mediasessd` service core:
   - create/register sessions per app
   - update metadata and playback state/position
   - accept control commands (play/pause/next/prev/seek/stop)
   - focus rules (deterministic):
     - a single `gain` owner at a time
     - new `gain` steals focus deterministically and forces prior `gain` to pause
     - `transient`/`duck` are modeled but must not claim real audio ducking (v1)
   - now-playing selection:
     - explicit `setActive(id)` wins
     - fallback: most recent playing/gain session
2. Eventing model for UI:
   - edge-triggered “changed” signal suitable for polling (deterministic)
   - `list()` snapshot returns stable ordering
3. Artwork cache (bounded):
   - compute stable key/hash for artwork URI/content
   - RAM cache always present; `/state` cache only when `/state` exists
   - deterministic eviction policy (LRU with stable tie-breakers)
4. Markers (rate-limited):
   - `mediasessd: ready`
   - `media: session create app=<id> sid=<sid>`
   - `media: focus grant sid=<sid> kind=gain`
   - `media: state sid=<sid> playing pos=<ms>`
   - `media: active sid=<sid>`
5. Deterministic host tests:
   - session lifecycle, focus steal rules, now-playing selection, control routing
   - artwork cache insert/evict behavior under caps

## Non-Goals

- Kernel changes.
- Real audio output or ducking (audiod handles audio later; v1 focus is state only).
- Rich media notification UI (wired later; optional gate in `TASK-0156`).

Follow-up note (v2):

- Multi-session handoff semantics, deterministic playback clock exposure, and a typed `playerctl` client are tracked as `TASK-0184`/`TASK-0185`.

## Constraints / invariants (hard requirements)

- Determinism: injected clock for position/freshness; stable ordering rules.
- Bounded buffers: cap metadata lengths, queue sizes, and cache bytes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: do not claim “ducked” audio behavior; only model focus state.

## Red flags / decision points (track explicitly)

- **RED (`/state` gating)**:
  - artwork cache persistence and any on-disk artifacts are gated on `TASK-0009`.
  - until then: RAM-only cache; no “persisted” markers.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p media_ux_v1_host -- --nocapture` (or equivalent crate name)
  - Required tests:
    - focus steal determinism
    - control routing to active session only
    - now-playing selection determinism
    - artwork cache bounded eviction determinism

## Touched paths (allowlist)

- `source/services/mediasessd/` (new or replace placeholder if exists)
- `tools/nexus-idl/schemas/media.capnp` (or existing schema location)
- `tests/media_ux_v1_host/` (new)
- `docs/media/overview.md` (added in `TASK-0156`)

## Plan (small PRs)

1. mediasessd schema + core state machine + markers
2. host tests (focus/control/now-playing/artcache)
3. docs stub (or defer docs to `TASK-0156` to keep PR small)

## Acceptance criteria (behavioral)

- Host tests deterministically validate sessions, focus, control routing, and bounded artwork caching.
