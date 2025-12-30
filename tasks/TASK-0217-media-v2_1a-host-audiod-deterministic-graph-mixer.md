---
title: TASK-0217 Media UX v2.1a (host-first): audiod deterministic audio graph/mixer (48k/10ms) + tone/WAV fixtures + per-session gain/mute + metrics + tests
status: Draft
owner: @media
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Audiod v16b baseline: tasks/TASK-0100-ui-v16b-audiod-mixer.md
  - Media sessions v2 semantics (clock/handoff): tasks/TASK-0184-media-ux-v2a-host-handoff-playerctl-deterministic-clock.md
  - Persistence substrate (/state for metrics export): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a QEMU-safe, deterministic “audio engine” that provides realistic behavior for Media UX and SystemUI
without touching real audio devices:

- deterministic graph + mixer,
- deterministic generators/fixtures (tone + PCM16 WAV),
- per-session volume/mute,
- deterministic metrics (peaks/clipping/active sessions),
- and host tests that prove behavior without wallclock flakiness.

This task is host-first. OS integration and UI wiring is v2.1b.

## Goal

Deliver:

1. `audiod` service (deterministic engine):
   - fixed output format:
     - 48 kHz, stereo PCM16, block = 480 frames (10ms)
   - per-session graph:
     - `Source -> Gain -> (Duck placeholder) -> Mix`
   - sources:
     - tone generator (sine, deterministic phase)
     - PCM16LE WAV loader for fixtures only (`pkg://fixtures/audio/*.wav`)
   - mixer:
     - deterministic saturating arithmetic
     - bounded ring buffer sink for tests (no device I/O)
2. API surface (Cap’n Proto IDL):
   - create/destroy/start/pause/seek/loadWav/playTone
   - setGainDb (Q8.8 dB), mute
   - metrics snapshot (peaks/clip)
3. Metrics:
   - host tests consume in-memory metrics snapshots deterministically
   - OS persistence to `state:/media/metrics.jsonl` is **gated** on `/state`:
     - without `/state`, metrics export must be disabled or explicit `stub/placeholder` (no “written ok” claims)
4. Deterministic host tests `tests/media_v2_1_audio_host/`:
   - tone plays for N blocks and produces non-zero peaks
   - pause stops progression deterministically
   - gain changes peak magnitude within tolerance
   - mute yields peak=0
   - WAV load/play advances deterministic position in blocks
   - limits (max sessions) enforced deterministically

## Non-Goals

- Kernel changes.
- Real ALSA/JACK output.
- "Perfect" psychoacoustic correctness; this is a deterministic stub engine.
- I²S/codec device layer stubs or mediasessiond hooks (handled by `TASK-0254`/`TASK-0255` as an extension; this task focuses on session/graph based mixer).

## Constraints / invariants (hard requirements)

- Determinism:
  - no wallclock dependence in tests (inject clock and step blocks)
  - stable rounding rules for gain and mixing
- Bounded memory: per-session buffers and ring sizes are capped.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **YELLOW (overlap with TASK-0100)**:
  - `TASK-0100` defines a stream-based mixer. v2.1a is session/graph based and tuned for Media UX.
  - We should either:
    - unify into one `audiod` contract, or
    - explicitly keep the older stream API as “legacy v0” and document the bridge.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p media_v2_1_audio_host -- --nocapture`

## Touched paths (allowlist)

- `source/services/audiod/`
- `tools/nexus-idl/schemas/audio.capnp` (or canonical schema location)
- `pkg://fixtures/audio/`
- `tests/media_v2_1_audio_host/`
- docs may land in v2.1b

## Plan (small PRs)

1. audiod engine core + deterministic tick loop + markers
2. tone generator + WAV loader fixtures + tests
3. per-session gain/mute + metrics + tests

## Acceptance criteria (behavioral)

- Host tests deterministically validate audiod mixing/gain/mute/position and bounded behavior.
