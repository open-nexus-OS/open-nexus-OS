---
title: TASK-0099 UI v16a: media decoders (WAV/Vorbis + GIF/APNG/MJPEG) for QEMU-safe CPU playback/rendering
status: Draft
owner: @media
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusMedia SDK track (audio/video/image): tasks/TRACK-NEXUSMEDIA-SDK.md
  - UI renderer baseline: tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - SVG pipeline (for APNG/GIF frame blit patterns): tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
---

## Context

UI v16 needs a QEMU-safe media stack without hardware decode. We start with pure-Rust decoders:

- audio: WAV (PCM) + Ogg Vorbis → PCM frames, resampled to a target spec,
- video (v1 scope): GIF/APNG/MJPEG → BGRA frames.

Mixer, sessions, and apps are separate tasks.

## Goal

Deliver:

1. `userspace/media/decoders` crate:
   - WAV reader (PCM)
   - Ogg Vorbis decoder (pure Rust)
   - resampler to target rate/channels (deterministic)
   - GIF/APNG frame iterator to BGRA
   - MJPEG multipart parser to per-frame JPEG decode (pure Rust)
   - pull-based API returning `MediaPacket::{Pcm, VideoFrame}`
2. Markers:
   - `decoder: wav/ogg/gif/apng/mjpeg on`
3. Host tests proving determinism:
   - fixture decode checksums and frame counts stable.

## Non-Goals

- Kernel changes.
- MP4/H264/AV1, and full video/audio sync.

## Constraints / invariants

- Deterministic decode and resample results for fixtures.
- Bounded decode:
  - cap max pixels for video frames,
  - cap max packet sizes,
  - cap total decoded bytes per test run.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v16a_host/`:

- WAV and Vorbis decode fixtures → PCM checksum stable; resampled length matches expected
- GIF/APNG/MJPEG fixtures → deterministic frame count; first/last frame checksums stable

## Touched paths (allowlist)

- `userspace/media/decoders/` (new)
- `tests/ui_v16a_host/`
- `docs/media/decoders.md` (new)

## Plan (small PRs)

1. implement WAV + Vorbis + resampler + tests
2. implement GIF/APNG/MJPEG + tests
3. docs + marker
