---
title: TASK-0102 UI v16d: Music app + Video app (gif/apng/mjpeg) + MIME wiring + OS selftests/postflight
status: Draft
owner: @media
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusMedia SDK track (audio/video/image): tasks/TRACK-NEXUSMEDIA-SDK.md
  - Media apps product track (quick players vs library/hubs): tasks/TRACK-MEDIA-APPS.md
  - Media decoders: tasks/TASK-0099-ui-v16a-media-decoders.md
  - Audiod mixer: tasks/TASK-0100-ui-v16b-audiod-mixer.md
  - Media sessions + SystemUI controls: tasks/TASK-0101-ui-v16c-media-sessions-systemui-controls.md
  - MIME/content foundations: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Doc picker (open): tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With decoders + mixer + sessions in place, we can ship user-facing apps and QEMU proofs:

- Music app (quick player) for WAV/OGG background playback,
- Video app (quick player) for GIF/APNG/MJPEG only (no audio),
- MIME handler registration and Files/Picker “Open With” integration.

Scope note:

- Media UX v1 provides a deterministic “sample player” app and media controls proof earlier (`TASK-0156`).
  This task focuses on **quick players** and decoder/mixer integration.
- The Apple Music-style **Music library app** (Listen Now/Browse/Library/Search + provider sign-in) is tracked in
  `tasks/TRACK-MEDIA-APPS.md` and is intentionally **out of scope** here.
- The Apple TV-style **TV hub app** (Watch Now/Library/Providers/Search + curated library) is tracked in
  `tasks/TRACK-MEDIA-APPS.md` and is intentionally **out of scope** here.

## Goal

Deliver:

1. `userspace/apps/music`:
   - open `audio/wav` and `audio/ogg` via picker/Open With
   - playlist view (minimal) and play/pause/seek stubs
   - streams PCM to `audiod`
   - registers media session and updates metadata/state
   - notification actions (play/pause/next) if available
   - markers:
     - `music: open uri=...`
     - `music: play`
     - `music: pause`
     - `music: finished`
2. `userspace/apps/video`:
   - open `image/gif`, `image/apng`, `multipart/x-mixed-replace` (MJPEG) via picker/Open With
   - vsync-paced frame iteration and blit
   - frame export as PNG (optional)
   - markers:
     - `video: open uri=... kind=gif|apng|mjpeg`
     - `video: frame n=...`
3. MIME wiring:
   - register handlers in `mimed` for audio and supported video kinds
   - Files context menu actions (optional) and picker filters (optional)
4. Host tests (minimal) and OS selftests/postflight markers for QEMU.

## Non-Goals

- Kernel changes.
- Real audio device output or HW video decode.

## Constraints / invariants

- Deterministic playback behavior for fixtures (bounded).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Bounded decode and buffer sizes per app.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v16d_host/`:

- music: decode fixture and push to a mocked audiod sink; markers emitted
- video: iterate fixture frames and produce deterministic frame count/checksum

### Proof (OS/QEMU) — gated

UART markers:

- `audiod: ready`
- `mediasessd: ready`
- `music: play`
- `SELFTEST: ui v16 audio ok`
- `SELFTEST: ui v16 session ok`
- `video: open uri=... kind=gif`
- `SELFTEST: ui v16 video ok`

## Touched paths (allowlist)

- `userspace/apps/music/` (new)
- `userspace/apps/video/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v16.sh` (delegates)
- `docs/apps/music.md` + `docs/apps/video.md` (new)

## Plan (small PRs)

1. music app skeleton + audiod + mediasess integration + markers
2. video app skeleton + frame blit + markers
3. MIME wiring + host tests + OS selftests + docs + postflight
