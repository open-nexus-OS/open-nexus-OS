---
title: TASK-0100B UI v16b follow-up: Audio Mixer DSL app/SystemUI surface + audiod bridge + host tests
status: Draft
owner: @ui @media
created: 2026-03-28
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Audiod service baseline: tasks/TASK-0100-ui-v16b-audiod-mixer.md
  - SystemUI DSL phase 1: tasks/TASK-0119-systemui-dsl-migration-phase1a-launcher-qs-host.md
  - DSL App Platform v1: tasks/TASK-0122B-dsl-app-platform-v1-shell-routing-launch-contract.md
---

## Context

`TASK-0100` intentionally focuses on the audio service and test sink. That keeps the service honest, but it leaves the
user-facing mixer UI unspecified. To test per-app volume/mute and later media integration in a realistic way, we need a
visible DSL surface that talks to `audiod`.

## Goal

Deliver:

1. Audio Mixer DSL UI:
   - per-app stream list
   - volume sliders
   - mute toggles
   - VU meter / level readout using `mixLevel`
2. Placement:
   - can be mounted as a SystemUI surface and/or launched as a standalone app
3. Deterministic bridge:
   - `audiod` adapters for list/setVolume/setMute/mixLevel
4. Host tests:
   - slider/mute actions round-trip deterministically against mocked `audiod`

## Non-Goals

- Replacing `audiod`.
- Real audio device output.
- Media session transport logic.

## Constraints / invariants (hard requirements)

- UI must remain a consumer of `audiod`, not a second mixer.
- Service IO only in effects/bridge calls.
- Deterministic VU fixtures in host tests.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- mixer UI snapshots
- volume/mute interactions deterministic
- VU readout stable under fixtures

### Proof (OS/QEMU) — gated

- visible mixer surface shows audiod state and can change at least one stream volume deterministically

## Touched paths (allowlist)

- DSL mixer UI package(s)
- SystemUI or launcher integration points
- `tests/ui_v16b_mixer_host/` (new)
- `docs/media/audiod.md`

## Plan (small PRs)

1. audiod DSL bridge
2. mixer UI
3. host tests + OS mounting follow-up
