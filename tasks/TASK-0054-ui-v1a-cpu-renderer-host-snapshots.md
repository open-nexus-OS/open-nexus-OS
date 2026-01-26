---
title: TASK-0054 UI v1a (host-first): BGRA8888 CPU renderer + damage tracking + headless snapshots (PNG/SSIM)
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI consumer of buffer/sync contracts: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers (future vsync spine): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
---

## Context

We need the first UI slice to be QEMU-tolerant and deterministic. The easiest way to build confidence
without kernel/display drivers is:

- a CPU renderer that draws into BGRA8888 buffers,
- stable “headless snapshot” tests on host,
- explicit damage tracking that later feeds a compositor.

This task is **host-first**. The OS compositor + surface IPC + VMO buffer sharing are in `TASK-0055`.

Scope note:

- Renderer Abstraction v1 (`TASK-0169`/`TASK-0170`) supersedes the “ad-hoc cpu renderer” direction by introducing
  a stable Scene-IR + Backend trait with a deterministic cpu2d backend and goldens.
  If `TASK-0169` lands, this task should be treated as “implemented by” that work (avoid parallel renderer crates).

## Goal

Deliver:

1. `userspace/ui/renderer` crate:
   - BGRA8888 framebuffer operations
   - text rendering with a single embedded fallback font
   - damage tracking (dirty rect accumulation)
2. `tests/ui_host_snap/`:
   - render fixed scenes
   - write PNGs and compare against goldens (pixel-exact first; SSIM optional follow-up)
3. Deterministic marker strings for later OS bring-up (not required to run in v1a).

## Non-Goals

- Kernel changes.
- A compositor.
- GPU acceleration.
- Input routing.

## Constraints / invariants (hard requirements)

- Deterministic output for a fixed seed:
  - no time-based randomness,
  - stable font rasterization parameters,
  - stable pixel format and stride rules.
- Pixel format: **BGRA8888**.
- Stride alignment: 64-byte aligned rows (documented and tested).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No heavy dependency chains; keep renderer small and auditable.

## Alignment with `TRACK-DRIVERS-ACCELERATORS`

- **Buffers**: treat “framebuffers” as VMO/filebuffer-backed memory (even on host we can emulate VMO maps).
- **Sync**: do not invent new fence semantics in v1a; renderer is pure compute.
- **Budgets**: enforce hard bounds on image sizes and allocations.

## Stop conditions (Definition of Done)

### Proof — required (host)

`cargo test -p ui_host_snap` green:

- renderer draws expected pixels for:
  - clear
  - rect
  - rounded-rect (simple coverage)
  - blit (from an in-memory image)
  - text (hello world)
- damage tracking:
  - rect ops add expected damage boxes
  - multiple ops coalesce/limit rect count deterministically
- snapshot tests:
  - produce PNGs
  - compare to goldens (pixel-exact or SSIM threshold if implemented)

## Touched paths (allowlist)

- `userspace/ui/renderer/` (new crate)
- `userspace/ui/fonts/` (embedded fallback font data)
- `tests/ui_host_snap/` (new)
- `docs/dev/ui/testing.md` (new)

## Plan (small PRs)

1. **Renderer core**
   - `Frame` with BGRA8888 pixels and stride
   - primitives: clear/rect/round_rect/blit/text
   - `Damage` accumulator with bounded rect count (e.g., SmallVec<[IRect; 4]>)

2. **Host snapshot tests**
   - goldens stored under `tests/ui_host_snap/goldens/`
   - deterministic rendering inputs (fixed font, fixed rasterization settings)
   - comparison:
     - pixel-exact first
     - optional SSIM tolerance as follow-up if minor differences are unavoidable across platforms

3. **Docs**
   - how to update goldens
   - how to add new cases
