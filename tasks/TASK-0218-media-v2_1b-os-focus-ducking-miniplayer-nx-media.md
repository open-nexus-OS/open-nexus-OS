---
title: TASK-0218 Media UX v2.1b (OS/QEMU): focus/ducking policies + per-app volume/mute + mini-player + nx-media + fixtures + selftests/docs (deterministic audio stub)
status: Draft
owner: @media
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Media apps product track (system-wide media focus/controls): tasks/TRACK-MEDIA-APPS.md
  - Audiod v2.1 host engine: tasks/TASK-0217-media-v2_1a-host-audiod-deterministic-graph-mixer.md
  - Media UX v2 core semantics: tasks/TASK-0184-media-ux-v2a-host-handoff-playerctl-deterministic-clock.md
  - Media UX v2 OS mini-player baseline: tasks/TASK-0185-media-ux-v2b-os-miniplayer-session-switch-notifs-selftests.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy caps baseline: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Media UX v2 provides deterministic session semantics and a mini-player UI, but explicitly does not include
real audio mixing/output.

Media UX v2.1 adds a deterministic audio engine stub (`audiod`) and integrates it with session control:

- focus/ducking policies,
- per-app volume/mute,
- and a mini-player that can control and observe audio state deterministically.

All behavior remains offline and QEMU-tolerant; no device audio output is required.

## Goal

Deliver:

1. `mediasessd` ↔ `audiod` integration:
   - sessions published in mediasessd map to audio sessions in audiod
   - controls (play/pause/seek/vol/mute) route deterministically to audiod
   - deterministic position:
     - source of truth is audiod block progression (not wallclock)
2. Focus/ducking policy (deterministic):
   - define focus classes (music/comms/alarm/ui) and deterministic rules:
     - alarm preempts (others paused)
     - comms ducks music/ui by `duck_db`
     - ui transient ducks music by `duck_db_ui`
     - auto-resume behavior is deterministic and schema-controlled
   - ramping uses block-based steps (no wallclock):
     - `ramp_ms` converted to a fixed number of 10ms blocks
3. Per-app volume/mute:
   - volume is per session/app overlay
   - mute per session
4. SystemUI:
   - mini-player shows current session, play/pause, seek, per-app volume/mute
   - shows “DUCKED” indicator when applicable
   - notifications remain gated on notif actions tasks (as in `TASK-0185`)
5. CLI `nx-media`:
   - list/current/play/pause/seek/volume/mute
   - deterministic tone/load helpers for test fixtures (host tool; QEMU selftests must not depend on running it inside QEMU)
6. Fixtures:
   - deterministic PCM16LE WAV fixtures under `pkg://fixtures/audio/` and artwork URIs
7. OS selftests (bounded):
   - `SELFTEST: media play ok`
   - `SELFTEST: media duck ok`
   - `SELFTEST: media mute ok`
   - `SELFTEST: media focus resume ok`
   - proofs must validate via service state/metrics (peaks/position), not log greps
8. Schema + caps:
   - `schemas/media_v2_1.schema.json`:
     - duck_db values, ramp blocks, session limits, engine constants
   - caps:
     - `media.session.publish`, `media.session.control`, `media.volume.set`, `media.mute.set`
   - `/state` gating:
     - audiod metrics export to `state:/media/metrics.jsonl` requires `/state`; without it, export disabled or explicit placeholder
9. Docs:
   - deterministic engine design and limitations
   - focus/ducking rules
   - nx-media usage
   - testing/markers

## Non-Goals

- Kernel changes.
- Real audio device output.
- Full decode pipeline (only PCM16 fixtures + tone generator in this step).

## Constraints / invariants (hard requirements)

- Determinism: block-based timing; stable ordering; injected clocks only in tests.
- No fake success markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p media_v2_1_audio_host -- --nocapture` (from v2.1a)
  - `cargo test -p media_ux_v2_host -- --nocapture` (from v2)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: media play ok`
    - `SELFTEST: media duck ok`
    - `SELFTEST: media mute ok`
    - `SELFTEST: media focus resume ok`

## Touched paths (allowlist)

- `source/services/audiod/` + `audio.capnp`
- `source/services/mediasessd/` (integration)
- `userspace/systemui/tray/mini_player/` (extend)
- `tools/nx-media/`
- `pkg://fixtures/audio/`
- `source/apps/selftest-client/`
- `schemas/media_v2_1.schema.json`
- `docs/media/` + `docs/tools/nx-media.md` + `docs/ui/testing.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. mediasessd↔audiod mapping + deterministic focus/ducking policy
2. mini-player volume/mute + duck indicator
3. nx-media extensions + fixtures
4. OS selftests + docs + postflight wrapper (delegating)

## Acceptance criteria (behavioral)

- In QEMU, deterministic audio stub + focus/ducking + per-app volume/mute are proven via selftest markers and audiod metrics.
