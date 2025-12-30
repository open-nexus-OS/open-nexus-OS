---
title: TASK-0105 UI v17c: screen recorder (recorderd) via screencapd + capture overlay + privacy chips + NXC container
status: Draft
owner: @media
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Perms/privacy substrate: tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - Screen capture substrate: tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md
  - Mic input: tasks/TASK-0104-ui-v17b-camerad-micd-virtual-sources.md
  - Media decoders (optional): tasks/TASK-0099-ui-v16a-media-decoders.md
  - Share sheet (after stop): tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md
---

## Context

We want a QEMU-friendly screen recorder:

- video: MJPEG from `screencapd` frames,
- audio: optional PCM (or Ogg later) from `micd`,
- output: a simple container stored under `/state/captures`.

We also need a SystemUI capture overlay and privacy indicator chips.

Camera app and Gallery integration are separate tasks.

## Goal

Deliver:

1. `recorderd` service:
   - `start(appId, withAudio)`, `status`, `stop`
   - permission enforcement:
     - requires `screen` grant
     - if audio: requires `microphone` grant
   - uses `screencapd` for frames and encodes MJPEG
   - reads PCM windows from `micd` when enabled
   - writes `state:/captures/<ts>-capture.nxc`:
     - NXC = zip with `manifest.json`, `video.mjpeg`, and optional `audio.wav` (v1)
   - emits privacy indicator events for `screen`
   - markers:
     - `recorderd: ready`
     - `record: start (audio=bool)`
     - `record: stop (uri=..., bytes=...)`
2. SystemUI capture overlay:
   - start/stop recording, audio toggle, countdown stub
   - privacy chips from `privacyd` stream
   - post-stop share sheet entry for the capture URI
   - markers:
     - `capture: ui open`
     - `capture: recording on`
     - `capture: recording off`
3. Host tests for container validity and frame counts.

## Non-Goals

- Kernel changes.
- Full MP4 container.
- Hardware accelerated capture/encode.

## Constraints / invariants

- Deterministic recording for fixture scenes (bounded duration and fps).
- Bounded disk usage (cap capture size and duration).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v17c_host/`:

- record a 2s deterministic capture:
  - manifest valid
  - MJPEG frame count ≈ fps×dur (within tolerance)
  - audio section present when enabled

## Touched paths (allowlist)

- `source/services/recorderd/` (new)
- SystemUI capture overlay (new)
- `tests/ui_v17c_host/`
- `docs/media/recorder.md` (new)

## Plan (small PRs)

1. recorderd core + NXC container writer + markers
2. SystemUI capture overlay + privacy chips + markers
3. host tests + docs

