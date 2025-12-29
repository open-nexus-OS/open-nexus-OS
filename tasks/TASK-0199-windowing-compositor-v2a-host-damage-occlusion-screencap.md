---
title: TASK-0199 Windowing/Compositor v2a (host-first): damage regions + occlusion culling + input regions hit-test + deterministic screencap/thumb + goldens
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Renderer abstraction (Scene-IR + cpu2d goldens): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - windowd↔renderer OS wiring: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Present scheduler + input routing baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Tiled compositor/occlusion deluxe: tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - WM controller baseline: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - Screenshot/share baseline: tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want a “v2” windowing/compositor correctness & performance slice that is testable host-first:

- precise damage tracking (rect/region),
- occlusion culling based on opaque regions,
- input hit-testing using explicit input regions,
- deterministic screenshots and window thumbnails for UI overlays.

The repo already has future work that overlaps (UI v2a/v4a/v6a/v7c). This task is a focused integration subset
that produces deterministic host proofs and stable primitives that OS wiring can reuse (v2b).

## Goal

Deliver:

1. Core geometry primitives (if not already present in renderer stack):
   - `IRect`, `Region` with deterministic ops:
     - union/intersect/subtract
     - clamp to display bounds
   - deterministic caps:
     - max rect count per region (spill to Full)
2. Compositor-side data model (host-first “windowd model” crate or windowd core module):
   - `Layer` enum (Background/App/Overlay/SystemTop)
   - `SurfaceDesc { layer, bounds, input_region, opaque_region, z }`
   - damage submission:
     - `Damage::Rect|Region|Full`
   - stable ordering: `(layer asc, z asc, id asc)`
3. Damage tracking & occlusion:
   - accumulate damage until vsync tick
   - compute minimal compose region (damage union)
   - occlusion culling using opaque regions top→bottom (deterministic region math)
   - provide deterministic metrics for tests:
     - tiles_redrawn, occluded_px, damage_rects
4. Input routing:
   - hit-testing walks top→bottom and respects `input_region`
   - pass-through overlays are supported by narrowing input regions
5. Deterministic screencap/thumb pipeline (host-first):
   - encode composed BGRA to PNG deterministically (fixed zlib level, no timestamps)
   - thumbnails are deterministic downscale (nearest or fixed kernel; pick one and lock it)
6. Host tests (`tests/windowing_v2_host/`):
   - damage union correctness (move window, union old/new minus overlap)
   - occlusion culling metrics (occluded area skipped)
   - input routing (overlay visuals but pass-through input region)
   - screencap golden PNG hash
   - thumb golden PNG hash

## Non-Goals

- Kernel changes.
- Full WM UX (alt-tab, snapping) is v2b.
- Atlas caches, clip stack, dynamic 30/60Hz pacing (these are v4a+ goals).

## Constraints / invariants (hard requirements)

- Deterministic region math and stable ordering.
- Bounded processing: cap rect counts; cap surface count in tests.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p windowing_v2_host -- --nocapture`
  - Required:
    - goldens stable
    - metrics deterministic

## Touched paths (allowlist)

- compositor geometry/core module (new; exact placement decided in implementation)
- `userspace/libs/renderer/` and/or `userspace/libs/scene-ir/` (integration helpers)
- `tests/windowing_v2_host/`
- `docs/windowing/overview.md` (added in v2b or minimal here)

## Plan (small PRs)

1. region math + caps + host tests
2. compositor model + damage/occlusion + metrics + host tests
3. input routing + host tests
4. deterministic screencap/thumb + goldens

## Acceptance criteria (behavioral)

- Host goldens and metrics prove that damage/occlusion/input routing work deterministically.
