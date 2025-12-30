---
title: TASK-0104 UI v17b: virtual camerad + synthetic micd with permission enforcement + privacy indicators
status: Draft
owner: @media
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Perms/privacy substrate: tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - Media decoders (optional fixtures): tasks/TASK-0099-ui-v16a-media-decoders.md
---

## Context

To enable capture features in QEMU without hardware, we provide synthetic capture devices:

- `camerad`: virtual BGRA frames from deterministic sources,
- `micd`: synthetic PCM sources (silence/tone/noise; loopback optional).

Both must enforce permissions and trigger privacy indicators.

Recorder and UI/app integration are separate tasks.

## Goal

Deliver:

1. `camerad` service:
   - sources: `test-pattern`, `slideshow(pkg://samples/cam/*.png)`, `solid(color)`
   - `open/nextFrame/close` API returning BGRA VMO frames
   - enforces `permsd.check(appId,camera)` before open
   - emits privacy on/off to `privacyd`
   - markers:
     - `camerad: ready`
     - `camera: open (app=... src=...)`
     - `camera: frame (sid=... ts=...)` (rate-limited)
2. `micd` service:
   - sources: `silence`, `tone(440Hz)`, `noise`, `loopback` (optional)
   - `open/read/close` API returning PCM bytes
   - enforces `permsd.check(appId,microphone)` before open
   - emits privacy to `privacyd`
   - markers:
     - `micd: ready`
     - `mic: open (app=... src=...)`
3. Host tests:
   - camera frames checksums stable for first N frames
   - mic tone RMS within expected band; silence is zero; noise is deterministic if seeded (otherwise RMS bounds only)
   - permission denied path is deterministic

## Non-Goals

- Kernel changes.
- Real V4L2/ALSA devices.

## Constraints / invariants

- Deterministic sources (test-pattern and slideshow).
- Bounded output sizes (cap max w/h/fps and frames per request).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v17b_host/`:

- permission denied without grant
- camera open + first 3 frame checksums stable
- mic tone produces RMS > threshold; silence RMS == 0

## Touched paths (allowlist)

- `source/services/camerad/` (new)
- `source/services/micd/` (new)
- `tests/ui_v17b_host/`
- `docs/media/camera.md` + `docs/media/mic.md` (new)

## Plan (small PRs)

1. camerad core + sources + markers + tests
2. micd core + sources + markers + tests
3. docs

