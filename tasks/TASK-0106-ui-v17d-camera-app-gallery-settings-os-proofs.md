---
title: TASK-0106 UI v17d: Camera app + Gallery integration + Settings privacy page + OS selftests/postflight
status: Draft
owner: @media
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Perms/privacy substrate: tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - Camera/mic devices: tasks/TASK-0104-ui-v17b-camerad-micd-virtual-sources.md
  - Screen recorder + capture UI: tasks/TASK-0105-ui-v17c-screen-recorder-capture-overlay.md
  - Thumbnailer + recents: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - Files integration: tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

After perms/privacy + camerad/micd + recorderd exist, we can ship user-facing apps:

- Camera app (photo + video modes on virtual camera sources),
- Gallery app integration for pictures/captures with thumbnails and share,
- Settings privacy page to manage grants.

This task owns OS/QEMU selftests and postflight markers for UI v17.

## Goal

Deliver:

1. `userspace/apps/camera`:
   - photo mode: capture BGRA frame from camerad and save PNG under `state:/pictures/Camera/`
   - video mode: record camera frames to MJPEG (audio optional via micd)
   - requests permissions via `permsd.request`
   - markers:
     - `camera: photo saved uri=...`
     - `camera: video saved uri=...`
2. `userspace/apps/gallery` (or extend existing):
   - browses `state:/pictures/` and `state:/captures/`
   - thumbnails via `thumbd`
   - opens capture items and supports share/delete/rename (minimal)
   - markers:
     - `gallery: index n=...`
     - `gallery: open uri=...`
3. Settings privacy page:
   - lists per-app grants (camera/mic/screen)
   - revoke buttons and global block toggles (if supported)
   - marker:
     - `settings: privacy revoke (app=... cap=...)`
4. OS selftests:
   - request perm, capture photo, record screen briefly, verify privacy indicators toggle
   - markers:
     - `SELFTEST: ui v17 photo ok`
     - `SELFTEST: ui v17 record ok`
     - `SELFTEST: ui v17 privacy ok`
5. Postflight script `postflight-ui-v17.sh` (delegating) and docs.

## Non-Goals

- Kernel changes.
- Real camera hardware.
- Full gallery metadata/EXIF.

## Constraints / invariants

- Deterministic capture sources for tests (test-pattern/slideshow).
- Bounded storage:
  - cap photo size and capture duration in selftests.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `permsd: ready`
- `privacyd: ready`
- `camerad: ready`
- `micd: ready`
- `recorderd: ready`
- `SELFTEST: ui v17 photo ok`
- `SELFTEST: ui v17 record ok`
- `SELFTEST: ui v17 privacy ok`

## Touched paths (allowlist)

- `userspace/apps/camera/` (new)
- `userspace/apps/gallery/` (new or extend)
- `userspace/apps/settings/` (privacy page extension)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v17.sh` (delegates)
- `docs/apps/camera.md` + `docs/apps/gallery.md` + `docs/privacy/overview.md` (extend) + `docs/ui/testing.md` (extend)

## Plan (small PRs)

1. camera app + photo mode + markers
2. camera video mode + mic optional + markers
3. gallery browse + thumbs + markers
4. settings privacy page + revoke + markers
5. OS selftests + postflight + docs

