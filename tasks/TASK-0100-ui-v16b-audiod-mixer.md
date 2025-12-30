---
title: TASK-0100 UI v16b: audiod software mixer (per-app streams, volume/mute, focus) + test sink + markers
status: Draft
owner: @media
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Audio focus policy (future): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Persistence (optional capture ring): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
---

## Context

We need a QEMU-safe audio path without real device output. The first step is a software mixer service:

- apps write PCM into per-app streams,
- audiod mixes to a single sink,
- sink is a stub (silent) and/or a deterministic capture buffer for tests.

Media sessions and apps are separate tasks.

## Goal

Deliver:

1. `audiod` service:
   - per-app stream model (open/write/pause/resume/close)
   - volume/mute per stream
   - mix tick loop to a single output spec (configurable)
   - VU levels (`mixLevel()`) for SystemUI OSD and tests
2. IDL `audio.capnp` with `openStream/write/setVolume/setMute/pause/resume/close/mixLevel`.
3. Test sink:
   - under host tests, capture mixed PCM into a ring buffer
   - optionally dump a WAV for debugging (not used as proof)
4. Markers:
   - `audiod: ready`
   - `audiod: stream open (app=...)`
   - `audiod: mix tick (vu=...)` (rate-limited)
5. Host tests for mixer behavior deterministically.

## Non-Goals

- Kernel changes.
- Real ALSA/JACK output.
- I²S/codec device layer stubs or mediasessiond hooks (handled by `TASK-0254`/`TASK-0255` as an extension; this task focuses on per-app stream mixer).

## Constraints / invariants

- Deterministic mix for fixture inputs (within fixed-point or defined float rounding).
- Bounded buffers:
  - cap per-stream queued bytes,
  - cap total mixed capture bytes in tests.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v16b_host/`:

- open stream, write PCM → mixLevel VU rises above threshold
- pause → VU drops near zero deterministically
- multiple streams mixed with volumes yields expected relative VU (tolerances documented)

## Touched paths (allowlist)

- `source/services/audiod/` (new)
- `source/services/audiod/idl/audio.capnp` (new)
- `tests/ui_v16b_host/`
- `docs/media/audiod.md` (new)

## Plan (small PRs)

1. audiod core + IDL + markers
2. test sink + host tests
3. docs
