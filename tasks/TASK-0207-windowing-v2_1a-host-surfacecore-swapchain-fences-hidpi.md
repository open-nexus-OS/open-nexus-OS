---
title: TASK-0207 Windowing/Compositor v2.1a (host-first): GPU-ready surface/swapchain contract + simulated timeline fences + vsync domains + HiDPI dp↔px mapping + deterministic tests
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Windowing/Compositor v2 integration: tasks/TASK-0199-windowing-compositor-v2a-host-damage-occlusion-screencap.md
  - Present scheduler + fences baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - windowd compositor surfaces baseline (VMO + minimal present fence): tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Renderer/windowd wiring baseline: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Driver contracts (future GPU): tasks/TRACK-DRIVERS-ACCELERATORS.md
---

## Context

We want the windowing surface model to be **GPU-ready** without requiring a GPU backend yet:

- swapchains (N images),
- acquire/release fences,
- multiple vsync domains,
- device-independent pixels (dp) with HiDPI scaling to physical pixels (px),
- all while keeping the current CPU renderer and deterministic host proofs.

This task is host-first and defines the core contracts and deterministic tests. OS/QEMU wiring is v2.1b.

## Goal

Deliver:

1. `userspace/libs/surfacecore`:
   - `PixelFormat` v1 (single format): `Rgba8888Premul`
   - `SwapchainDesc { images, fmt, w_px, h_px, vsync_domain }`
   - **Simulated timeline fences**:
     - avoid ad-hoc “event fence” semantics; model as monotonic fence IDs in a single timeline per swapchain
     - `wait(fence_id, timeout)` uses injected time in tests
   - CPU swapchain implementation for host tests:
     - N images backed by deterministic `Vec<u8>` with stable stride/alignment rules
     - safe accessors (no raw pointers in public API):
       - expose `&mut [u8]` for the acquired image
2. Vsync domains model (host simulation):
   - `VsyncDomain { id, hz, seq }` with deterministic tick simulation
   - latch rules:
     - compose only when new image submitted or an “always needs present” flag is set (cursor/animation)
3. HiDPI v1 mapping model:
   - `scale` chosen from allowlist (1.0/1.25/1.5/2.0)
   - rules:
     - app coordinates are dp
     - compositor operates in px
     - conversions are explicit and deterministic (rounding policy documented and tested)
4. Deterministic host tests `tests/windowing_v2_1_host/`:
   - fence ordering:
     - acquire→submit→latch→release ordering is enforced
   - triple-buffering does not deadlock under 120 frames
   - pacing:
     - no damage and no animation → skip compose deterministically
   - HiDPI mapping:
     - dp move produces expected px damage and hit-test mapping

## Non-Goals

- Kernel changes.
- Real GPU driver integration (future driver track).
- Multi-display enumeration (v2.1 models “domains” without requiring real multi-head).

## Constraints / invariants (hard requirements)

- Do not introduce a second sync model: fences must map cleanly to future timeline fences (driver track).
- Deterministic rounding and stable ordering.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (raw pointer API)**:
  - A `*mut u8` image pointer is easy to misuse and conflicts with the repo’s safety posture.
  - The contract should expose safe slices for CPU paths; OS paths use VMO mappings (v2.1b).

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p windowing_v2_1_host -- --nocapture`

## Touched paths (allowlist)

- `userspace/libs/surfacecore/` (new)
- `tests/windowing_v2_1_host/` (new)
- docs (may land in v2.1b)

## Plan (small PRs)

1. surfacecore swapchain + simulated timeline fences + tests
2. vsync domain simulation + pacing rules + tests
3. HiDPI dp↔px mapping helpers + tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove swapchain/fence ordering, pacing, and HiDPI mapping semantics.
