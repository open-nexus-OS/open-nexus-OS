---
title: TASK-0060 UI v4a: tiled compositor + occlusion + clip stack + atlases/caches + perf/pacing metrics
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v3b baseline (clip/scroll/effects): tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - UI v2a baseline (present scheduler): tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v2b baseline (shaping/svg): tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Drivers/Accelerators contracts: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Config broker (budgets): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (limits): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

UI v4 aims to lower CPU cost while keeping frames visibly smoother, without GPU drivers.
The core is compositor-side work reduction:

- region/tile-based damage (only redraw what changed),
- occlusion culling,
- clip hierarchies (effective clip propagation),
- atlas caches for glyph/SVG/blur masks with budgets and deterministic eviction,
- present pacing + basic perf markers (jank/idle mode switches).

This is v4a: compositor perf foundation. Gestures and accessibility semantics live in v4b (`TASK-0061`).

## Goal

Deliver:

1. Region primitives and tile mapping (default tile 64×64, configurable).
2. `windowd` tiled compositor:
   - tile set from damage region
   - occlusion culling (back-to-front covered region)
   - per-present markers: tiles and damaged pixels
3. Clip stack:
   - nested rect clip stack in layer tree
   - effective clip = intersection along ancestry
4. Atlases/caches with budgets:
   - glyph atlas (alpha8)
   - SVG raster cache (BGRA)
   - blur mask cache
   - strict budgets + LRU eviction + counters
5. Present pacing + perf/energy-ish markers:
   - target frame time alignment
   - dynamic 30/60Hz switching based on idle+damage thresholds
   - counters for jank misses and idle switches

Follow-ups for a full perf tracing + HUD + regression gates pipeline:

- `TASK-0143` (perfd tracer + Chrome Trace export)
- `TASK-0144` (instrumentation hooks + Perf HUD + nx-perf)
- `TASK-0145` (deterministic perf gates for key scenes)
6. Deterministic host tests and OS/QEMU markers.

## Non-Goals

- Kernel changes.
- Gesture recognition and accessibility (v4b).
- GPU backend (interfaces should not prevent it, but v4a proofs stay CPU-based).

## Constraints / invariants (hard requirements)

- Deterministic region math and tile selection.
- Strict budgets and bounded state:
  - cap region rect counts,
  - cap atlas pages and bytes,
  - rate-limit eviction markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v4a_host/`:

- region math goldens (union/intersect/subtract)
- tiling: damage → expected tile set
- occlusion: fully covered rects are skipped deterministically
- atlas: eviction order and hit/miss counters under budget pressure
- pacing: idle → 30Hz, input/damage → 60Hz (simulated time)

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: tiling on (tile=64)`
- `windowd: occlusion on`
- `windowd: clip stack on`
- `uiatlas: on (glyph=... svg=... blur=...)`
- `windowd: hz -> 30`
- `windowd: hz -> 60`
- `SELFTEST: ui v4 pacing ok`

## Touched paths (allowlist)

- `userspace/ui/renderer/` (region primitives)
- `userspace/ui/atlas/` (new)
- `source/services/windowd/` (tiling/occlusion/clip-stack/pacing)
- `tests/ui_v4a_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v4a.sh` (delegates)
- `docs/ui/compositor.md` + `docs/ui/atlas.md` (new)

## Plan (small PRs)

1. Region primitives + tile mapping helpers
2. windowd tiler + occlusion + markers
3. clip stack in layer tree + effective clip propagation
4. atlas caches (glyph/SVG/blur) + budgets + metrics
5. pacing + 30/60Hz mode switching + markers
6. tests + docs + postflight
