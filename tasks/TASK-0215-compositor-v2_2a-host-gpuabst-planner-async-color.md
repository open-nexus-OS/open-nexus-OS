---
title: TASK-0215 Compositor v2.2a (host-first): gpuabst CPU stub + plane planner (primary/overlay/cursor) + async present model + basic color spaces (sRGB/Linear) + deterministic tests
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Windowing v2.1 surfaces/fences baseline: tasks/TASK-0207-windowing-v2_1a-host-surfacecore-swapchain-fences-hidpi.md
  - Windowing v2 host compositor substrate: tasks/TASK-0199-windowing-compositor-v2a-host-damage-occlusion-screencap.md
  - Compositor caches/atlases (deluxe later): tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - Driver contracts (future GPU): tasks/TRACK-DRIVERS-ACCELERATORS.md
---

## Context

We want to make the compositor “GPU-ready” without needing a GPU in QEMU:

- introduce a GPU abstraction layer with a CPU stub backend,
- plan multi-plane composition (primary/overlay/cursor) deterministically,
- decouple composition from present (async present queue) with bounded latency,
- and add minimal color space plumbing (sRGB/Linear) with deterministic conversions.

This task is host-first: the contracts and deterministic tests come before OS wiring.

## Goal

Deliver:

1. `userspace/libs/gpuabst` (CPU stub backend):
   - `Device/Queue/Image` traits with a CPU implementation
   - `FenceId` is simulated (timeline-like) and deterministic under injected time in tests
   - `Op` set is small and maps to existing CPU renderer primitives
   - color space conversion support:
     - `ColorSpace::{Srgb, Linear}`
     - deterministic LUT-based conversions (256-entry LUT, explicit rounding)
2. Plane planner:
   - deterministic per-frame `Plan { primary, overlays, cursor }`
   - promotion heuristics:
     - cursor plane only for small cursor sprite surfaces
     - overlay promotion only for eligible opaque fullscreen surfaces (strict rules; deterministic)
   - emits debug markers/metrics in tests (not OS markers)
3. Async present model:
   - compose produces a `FrameBundle`
   - bounded present queue (size=3 default)
   - overflow rule: drop oldest, increment `dropped_frames` counter
   - release fence ordering is deterministic (no time-based assertions)
4. Deterministic host tests `tests/compositor_v2_2_host/`:
   - planner overlay promote yes/no (opaque vs translucent)
   - cursor plane selection and dp→px mapping
   - async present queue drops exactly one under overflow; fence ordering stable
   - color transform golden (linear gradient → sRGB hash)
   - metrics counters monotonic and deterministic

## Non-Goals

- Kernel changes.
- Real multi-plane hardware present (planner is prep only).
- Real GPU backend.

## Constraints / invariants (hard requirements)

- Determinism: injected time in tests; stable ordering; stable rounding for LUT conversions.
- Bounded memory: capped queue sizes and caps on ops/frame.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Stubs must be explicit: `gpuabst` CPU backend must not claim GPU acceleration.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p compositor_v2_2_host -- --nocapture`

## Touched paths (allowlist)

- `userspace/libs/gpuabst/` (new)
- compositor planner/async present module (placement decided in implementation)
- `tests/compositor_v2_2_host/`
- docs may land in v2.2b

## Plan (small PRs)

1. gpuabst traits + cpu backend + LUT conversions + tests
2. plane planner + heuristics + tests
3. async present queue + drop policy + tests
4. metrics snapshot types + tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove plane planning, async present drop policy, and sRGB/Linear conversions.
