---
title: TASK-0116 UI v20c: magnifier (lens/fullscreen) + color filters + high-contrast mode + prefs + tests
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v4a compositor baseline (post-process hooks): tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - Theme tokens baseline: tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
  - Prefs store: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
---

## Context

Accessibility display accommodations require compositor-level features:

- magnifier (lens and fullscreen),
- color filters (matrix transforms),
- high contrast mode (token set switch + enforcement).

This task is CPU-only and QEMU-safe: post-process runs in windowd/SystemUI composition path.

## Goal

Deliver:

1. Magnifier overlay (SystemUI):
   - lens mode following cursor
   - fullscreen mode with pan controls
   - prefs: mode and zoom
   - markers:
     - `magnifier: on (mode=lens|full)`
2. Color filters:
   - presets: grayscale, invert, deuteranopia, protanopia, tritanopia, contrast+
   - CPU color matrix post-process (deterministic)
   - marker: `colorfilter: on (preset=...)`
3. High contrast:
   - toggle to HC token set and enforce min contrast rule in UI kit (warn if violated)
   - marker: `highcontrast: on`
4. Host tests:
   - magnifier lens changes checksum deterministically for a fixture scene
   - color filter outputs stable histogram deltas or golden PNGs

## Non-Goals

- Kernel changes.
- GPU shaders.

## Constraints / invariants

- Deterministic post-processing math (integer or explicit rounding).
- Bounded cost:
  - cap magnifier zoom area and update rate.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v20c_host/`:

- apply each filter preset to a known fixture frame and validate checksum/golden
- magnifier lens sample produces deterministic output

### Proof (OS/QEMU) — gated

UART markers:

- `SELFTEST: ui v20 display ok` (owned by v20e)

## Touched paths (allowlist)

- SystemUI magnifier overlay
- windowd/systemui post-process pipeline (color matrix)
- `tests/ui_v20c_host/`
- `docs/a11y/vision.md` (new)

## Plan (small PRs)

1. color filter pipeline + presets + tests
2. magnifier overlay + prefs + tests
3. high contrast toggle + docs

