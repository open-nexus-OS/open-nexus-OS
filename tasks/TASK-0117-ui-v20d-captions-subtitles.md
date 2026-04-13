---
title: TASK-0117 UI v20d: captions/subtitles (SRT/VTT) crate + Video app rendering + SystemUI CC toggle + markers
status: Draft
owner: @media
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v1 DevX track: tasks/TRACK-DSL-V1-DEVX.md
  - DSL syntax/layout convention: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - Video app baseline: tasks/TASK-0102-ui-v16d-music-video-apps-os-proofs.md
  - Text shaping baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Prefs store: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Media sessions (CC hint wiring optional): tasks/TASK-0101-ui-v16c-media-sessions-systemui-controls.md
---

## Context

Captions are an accessibility feature and a media UX feature. We implement a small subtitles crate and integrate it
into the Video app and SystemUI overlays.

## Goal

Deliver:

1. `userspace/media/subtitles` crate:
   - parse SRT and minimal WebVTT into time-aligned cues
   - deterministic parsing and cue ordering
2. Video app integration:
   - visible video-player chrome is authored in the DSL and hosts caption toggles/settings there
   - timed caption rendering itself may use a blessed media/video surface layered under or with the video viewport
   - load sidecar captions alongside content URI (`.srt/.vtt`)
   - render captions with outline and configurable size/background
   - toggle CC via toolbar (persist via prefs)
   - markers:
     - `subtitles: loaded (cues=...)`
     - `video: cc on`
3. SystemUI CC toggle:
   - visible toggle should converge to DSL-authored SystemUI controls rather than one-off imperative media UI
   - global CC toggle (v1 can just set prefs; optional forward to active video session)
4. Host tests:
   - parse fixtures and verify cue counts and timestamps
   - at time T, renderer produces deterministic glyph box count (model-level) or snapshot PNG.

## Non-Goals

- Kernel changes.
- Full styling and positioning spec compliance.

## Constraints / invariants

- Deterministic parsing and render layout.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v20d_host/`:

- parse SRT/VTT fixtures and match expected cue count/time ranges
- simple rendering model produces deterministic output for a cue at time T

### Proof (OS/QEMU) — gated

UART markers:

- `subtitles: loaded (cues=...)`
- `video: cc on`
- `SELFTEST: ui v20 captions ok` (owned by v20e)

## Touched paths (allowlist)

- `userspace/media/subtitles/` (new)
- `userspace/apps/video/` (extend)
- SystemUI media overlay (CC toggle)
- `tests/ui_v20d_host/`
- `docs/media/subtitles.md` (new)

## Plan (small PRs)

1. subtitles parser crate + tests
2. video caption renderer + toggle + markers
3. SystemUI CC toggle + docs
